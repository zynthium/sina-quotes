use sina_quotes::SinaQuotes;
use std::time::Duration as StdDuration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().skip(1).collect();
    let symbols: Vec<String> = if args.is_empty() {
        vec!["hf_OIL".to_string(), "hf_GC".to_string()]
    } else {
        args
    };
    let symbol_refs: Vec<&str> = symbols.iter().map(|s| s.as_str()).collect();

    let client = SinaQuotes::builder()
        .http_timeout(StdDuration::from_secs(10))
        .default_data_length(100)
        .build()
        .await?;

    let mut streams = client.subscribe_quotes(&symbol_refs).await?;
    client.start_websocket(symbols.clone()).await?;

    let mut handles = Vec::new();
    for mut s in streams.drain(..) {
        let sym = s.symbol();
        let h = tokio::spawn(async move {
            let mut i = 0usize;
            while i < 20 {
                if s.changed().await.is_err() {
                    break;
                }
                let q = s.get();
                println!("[{} {:02}] {}", sym, i, q);
                i += 1;
            }
        });
        handles.push(h);
    }

    for h in handles {
        let _ = h.await;
    }

    client.close().await;
    Ok(())
}
