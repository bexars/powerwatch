//! Shared macOS backend: one GCD queue + IOKit power + Darwin notify.
//!
//! `IORegisterForSystemPower` does not deliver shutdown/restart, so
//! [`PowerEvent::Shutdown`] is never emitted here.

use flume::{Receiver, Sender};
use std::collections::HashMap;
use std::ffi::{CStr, c_int, c_void};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicI32, AtomicPtr, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use block2::{Block, RcBlock};
use dispatch2::{DispatchQueue, DispatchRetained};
use objc2_io_kit::{
    IO_OBJECT_NULL, IOAllowPowerChange, IODeregisterForSystemPower, IONotificationPort,
    IONotificationPortRef, IORegisterForSystemPower, IOServiceClose, io_object_t, io_service_t,
    kIOMessageCanSystemSleep, kIOMessageSystemHasPoweredOn, kIOMessageSystemWillSleep,
};

use crate::error::Error;
use crate::event::PowerEvent;

const LOCKED: &CStr = c"com.apple.screenIsLocked";
const UNLOCKED: &CStr = c"com.apple.screenIsUnlocked";
const QUEUE_LABEL: &str = "powerwatch";
const TOKEN_NONE: i32 = i32::MIN;

static RUNTIME: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);

pub(crate) struct Watch {
    runtime: Option<Arc<Runtime>>,
    id: u64,
}

struct Runtime {
    senders: Mutex<HashMap<u64, Sender<PowerEvent>>>,
    next_id: AtomicU64,
    root_port: AtomicU32,
    notify_port: AtomicPtr<c_void>,
    notifier: AtomicU32,
    lock_token: AtomicI32,
    unlock_token: AtomicI32,
    queue: DispatchRetained<DispatchQueue>,
}

unsafe extern "C" {
    fn notify_register_dispatch(
        name: *const i8,
        out_token: *mut c_int,
        queue: *mut c_void,
        handler: *mut Block<dyn Fn(c_int)>,
    ) -> u32;

    fn notify_cancel(token: c_int) -> u32;
}

pub(crate) fn start() -> Result<(Watch, Receiver<PowerEvent>), Error> {
    let (tx, rx) = flume::unbounded();
    let watch = subscribe(tx)?;
    Ok((watch, rx))
}

fn subscribe(tx: Sender<PowerEvent>) -> Result<Watch, Error> {
    let mut slot = RUNTIME.lock().unwrap_or_else(|poison| poison.into_inner());

    if let Some(existing) = slot.as_ref() {
        let id = existing.add_sender(tx);
        return Ok(Watch {
            runtime: Some(Arc::clone(existing)),
            id,
        });
    }

    let runtime = Runtime::register()?;
    let id = runtime.add_sender(tx);
    *slot = Some(Arc::clone(&runtime));
    Ok(Watch {
        runtime: Some(runtime),
        id,
    })
}

impl Runtime {
    fn register() -> Result<Arc<Self>, Error> {
        let queue = DispatchQueue::new(QUEUE_LABEL, None);
        let runtime = Arc::new(Self {
            senders: Mutex::new(HashMap::new()),
            next_id: AtomicU64::new(1),
            root_port: AtomicU32::new(IO_OBJECT_NULL),
            notify_port: AtomicPtr::new(std::ptr::null_mut()),
            notifier: AtomicU32::new(IO_OBJECT_NULL),
            lock_token: AtomicI32::new(TOKEN_NONE),
            unlock_token: AtomicI32::new(TOKEN_NONE),
            queue,
        });

        let refcon = Arc::as_ptr(&runtime).cast_mut().cast::<c_void>();
        let mut notify_port: IONotificationPortRef = std::ptr::null_mut();
        let mut notifier: io_object_t = IO_OBJECT_NULL;

        let root_port = unsafe {
            IORegisterForSystemPower(
                refcon,
                &mut notify_port,
                Some(power_callback),
                &mut notifier,
            )
        };

        if root_port == IO_OBJECT_NULL || notify_port.is_null() {
            return Err(Error::platform(
                "failed to register for system power notifications",
            ));
        }

        runtime.root_port.store(root_port, Ordering::Release);
        runtime
            .notify_port
            .store(notify_port.cast(), Ordering::Release);
        runtime.notifier.store(notifier, Ordering::Release);

        unsafe {
            IONotificationPort::set_dispatch_queue(notify_port, Some(&runtime.queue));
        }

        let queue_ptr = NonNull::from(&*runtime.queue).as_ptr().cast::<c_void>();
        let runtime_ptr = Arc::as_ptr(&runtime);

        let lock_block = RcBlock::new(move |_: c_int| {
            // SAFETY: cancelled in `teardown` before `Runtime` is freed.
            let runtime = unsafe { &*runtime_ptr };
            runtime.emit(PowerEvent::ScreenLocked);
        });
        let unlock_block = RcBlock::new(move |_: c_int| {
            let runtime = unsafe { &*runtime_ptr };
            runtime.emit(PowerEvent::ScreenUnlocked);
        });

        let mut lock_token = 0;
        let lock_status = unsafe {
            notify_register_dispatch(
                LOCKED.as_ptr(),
                &mut lock_token,
                queue_ptr,
                RcBlock::as_ptr(&lock_block).cast::<Block<dyn Fn(c_int)>>(),
            )
        };
        // notify retains the block; we leak our retain so the block stays
        // valid until `notify_cancel`.
        std::mem::forget(lock_block);

        if lock_status != 0 {
            runtime.teardown();
            return Err(Error::platform(
                "failed to register for screen lock notifications",
            ));
        }
        runtime.lock_token.store(lock_token, Ordering::Release);

        let mut unlock_token = 0;
        let unlock_status = unsafe {
            notify_register_dispatch(
                UNLOCKED.as_ptr(),
                &mut unlock_token,
                queue_ptr,
                RcBlock::as_ptr(&unlock_block).cast::<Block<dyn Fn(c_int)>>(),
            )
        };
        std::mem::forget(unlock_block);

        if unlock_status != 0 {
            runtime.teardown();
            return Err(Error::platform(
                "failed to register for screen lock notifications",
            ));
        }
        runtime.unlock_token.store(unlock_token, Ordering::Release);

        Ok(runtime)
    }

    fn add_sender(&self, tx: Sender<PowerEvent>) -> u64 {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut senders = self
            .senders
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        senders.insert(id, tx);
        id
    }

    fn remove_sender(&self, id: u64) {
        let mut senders = self
            .senders
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        senders.remove(&id);
    }

    fn emit(&self, event: PowerEvent) {
        let mut senders = self
            .senders
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        senders.retain(|_, tx| tx.send(event).is_ok());
    }

    fn teardown(&self) {
        let queue = self.queue.clone();
        queue.exec_sync(|| {
            let lock = self.lock_token.swap(TOKEN_NONE, Ordering::AcqRel);
            if lock != TOKEN_NONE {
                unsafe {
                    let _ = notify_cancel(lock);
                }
            }
            let unlock = self.unlock_token.swap(TOKEN_NONE, Ordering::AcqRel);
            if unlock != TOKEN_NONE {
                unsafe {
                    let _ = notify_cancel(unlock);
                }
            }

            let mut notifier = self.notifier.swap(IO_OBJECT_NULL, Ordering::AcqRel);
            if notifier != IO_OBJECT_NULL {
                unsafe {
                    let _ = IODeregisterForSystemPower(&mut notifier);
                }
            }

            let notify_port = self
                .notify_port
                .swap(std::ptr::null_mut(), Ordering::AcqRel)
                as IONotificationPortRef;
            if !notify_port.is_null() {
                unsafe {
                    IONotificationPort::set_dispatch_queue(notify_port, None);
                    IONotificationPort::destroy(notify_port);
                }
            }

            let root = self.root_port.swap(IO_OBJECT_NULL, Ordering::AcqRel);
            if root != IO_OBJECT_NULL {
                let _ = IOServiceClose(root);
            }
        });
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.teardown();
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let Some(runtime) = self.runtime.take() else {
            return;
        };
        runtime.remove_sender(self.id);
        let mut slot = RUNTIME.lock().unwrap_or_else(|poison| poison.into_inner());
        // slot + this Watch is the last pair: tear down before another start().
        if Arc::strong_count(&runtime) == 2
            && slot
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &runtime))
        {
            *slot = None;
        }
        drop(slot);
        drop(runtime);
    }
}

unsafe extern "C-unwind" fn power_callback(
    refcon: *mut c_void,
    _service: io_service_t,
    message_type: u32,
    message_argument: *mut c_void,
) {
    if refcon.is_null() {
        return;
    }
    let runtime = unsafe { &*refcon.cast::<Runtime>() };
    let root = runtime.root_port.load(Ordering::Acquire);
    let notification_id = message_argument as isize;

    if message_type == kIOMessageCanSystemSleep {
        if root != IO_OBJECT_NULL {
            let _ = IOAllowPowerChange(root, notification_id);
        }
        return;
    }

    if message_type == kIOMessageSystemWillSleep {
        if root != IO_OBJECT_NULL {
            let _ = IOAllowPowerChange(root, notification_id);
        }
        runtime.emit(PowerEvent::Suspend);
        return;
    }

    if message_type == kIOMessageSystemHasPoweredOn {
        runtime.emit(PowerEvent::Resume);
    }
}
