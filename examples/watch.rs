use std::sync::mpsc::TryRecvError;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (_watch, rx) = powerwatch::PowerWatch::start()?;
    loop {
        match rx.try_recv() {
            Ok(ev) => println!("{ev:?}"),
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => break,
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Ok(())
}
