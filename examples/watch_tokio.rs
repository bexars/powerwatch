#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut task = tokio::spawn(async {
        let (_watch, events) = powerwatch::PowerWatch::start()?;
        loop {
            match events.recv_async().await {
                Ok(ev) => println!("{ev:?}"),
                Err(_) => break,
            }
        }
        Ok::<_, powerwatch::Error>(())
    });

    tokio::select! {
        result = &mut task => {
            result??;
        }
        _ = tokio::signal::ctrl_c() => {
            task.abort();
        }
    }
    Ok(())
}
