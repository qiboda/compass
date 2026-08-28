use std::process::ExitCode;

use compass_collectors::block_trade;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run_cli(&args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

async fn run_cli(args: &[String]) -> Result<(), Box<dyn std::error::Error>> {
    let Some(command) = args.first() else {
        print_usage();
        return Ok(());
    };
    match command.as_str() {
        "block-trade" | "block_trade" => {
            let mut start = None;
            let mut end = None;
            let mut years: Option<Vec<i32>> = None;
            let mut page_size = 100usize;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--start" => {
                        start = Some(args.get(i + 1).ok_or("--start requires a value")?.as_str());
                        i += 2;
                    }
                    "--end" => {
                        end = Some(args.get(i + 1).ok_or("--end requires a value")?.as_str());
                        i += 2;
                    }
                    "--years" => {
                        let raw = args.get(i + 1).ok_or("--years requires a value")?;
                        years = Some(
                            raw.split(',')
                                .filter_map(|s| s.trim().parse::<i32>().ok())
                                .collect(),
                        );
                        i += 2;
                    }
                    "--page-size" => {
                        let raw = args.get(i + 1).ok_or("--page-size requires a value")?;
                        page_size = raw.parse()?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let out = block_trade::run(years.as_deref(), start, end, page_size).await?;
            println!("{}", out.display());
            Ok(())
        }
        _ => {
            print_usage();
            Ok(())
        }
    }
}

fn print_usage() {
    eprintln!(
        "usage: compass-collectors block-trade [--start YYYY-MM-DD] [--end YYYY-MM-DD] [--years 2024,2025] [--page-size N]"
    );
}
