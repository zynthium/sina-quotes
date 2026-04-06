use clap::Parser;
use futures_util::StreamExt;
use sina_quotes::{fetch_history, poll_international, subscribe};
use std::time::SystemTime;

const INTERNATIONAL_FUTURES: &[&str] = &[
    "hf_FEF",
    "hf_FCPO",
    "hf_RSS3",
    "hf_RS",
    "hf_BTC",
    "hf_CT",
    "hf_NID",
    "hf_PBD",
    "hf_SND",
    "hf_ZSD",
    "hf_AHD",
    "hf_CAD",
    "hf_S",
    "hf_W",
    "hf_C",
    "hf_BO",
    "hf_SM",
    "hf_TRB",
    "hf_HG",
    "hf_NG",
    "hf_CL",
    "hf_SI",
    "hf_GC",
    "hf_LHC",
    "hf_OIL",
    "hf_XAU",
    "hf_XAG",
    "hf_XPT",
    "hf_XPD",
    "hf_EUA",
];

fn get_random_symbols(n: usize) -> Vec<String> {
    use std::collections::HashSet;
    let mut seen = HashSet::new();
    let mut result = Vec::with_capacity(n);
    
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as usize;
    
    let mut idx = seed;
    while result.len() < n {
        let symbol = INTERNATIONAL_FUTURES[idx % INTERNATIONAL_FUTURES.len()];
        if seen.insert(symbol) {
            result.push(symbol.to_string());
        }
        idx = idx.wrapping_mul(31).wrapping_add(1);
    }
    
    result
}

#[derive(Parser, Debug)]
#[command(name = "sina-quotes")]
enum Cmd {
    History {
        symbol: String,
        #[arg(default_value = "5")]
        period: u32,
    },
    Subscribe {
        symbols: Vec<String>,
    },
    PollIntl {
        symbols: Vec<String>,
        #[arg(short, long, default_value = "2")]
        interval: u64,
    },
    RandomIntl {
        #[arg(default_value = "5")]
        count: usize,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    match Cmd::parse() {
        Cmd::History { symbol, period } => {
            tracing::info!("fetching {symbol} {period}min history...");
            let bars = fetch_history(&symbol, period, 3).await?;
            tracing::info!("got {} records", bars.len());
            for bar in bars.iter().take(5) {
                println!("{:?}", bar);
            }
            if bars.len() > 5 {
                println!("... and {} more", bars.len() - 5);
            }
        }
        Cmd::Subscribe { symbols } => {
            let sym_refs: Vec<&str> = symbols.iter().map(|s| s.as_str()).collect();
            tracing::info!("subscribing to {:?}", sym_refs);

            let mut stream = subscribe(&sym_refs).await?;
            for _ in 0..10 {
                if let Some(msg) = stream.next().await {
                    println!("{:?}", msg?);
                }
            }
        }
        Cmd::PollIntl { symbols, interval } => {
            let sym_refs: Vec<&str> = symbols.iter().map(|s| s.as_str()).collect();
            tracing::info!("polling (intl) {:?} every {}s", sym_refs, interval);

            let mut stream = poll_international(&sym_refs, interval).await?;
            for i in 0..10 {
                if let Some(msg) = stream.next().await {
                    match msg {
                        Ok(quote) => println!("[{}] {:?}", i + 1, quote),
                        Err(e) => tracing::error!("error: {:?}", e),
                    }
                }
            }
        }
        Cmd::RandomIntl { count } => {
            let symbols = get_random_symbols(count);
            println!("Selected intl symbols: {:?}", symbols);
            println!();
            
            let sym_refs: Vec<&str> = symbols.iter().map(|s| s.as_str()).collect();
            tracing::info!("polling (intl) {:?} every 2s", sym_refs);

            let mut stream = poll_international(&sym_refs, 2).await?;
            for i in 0..10 {
                if let Some(msg) = stream.next().await {
                    match msg {
                        Ok(quote) => println!("[{}] {:?}", i + 1, quote),
                        Err(e) => tracing::error!("error: {:?}", e),
                    }
                }
            }
        }
    }

    Ok(())
}