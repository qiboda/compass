use std::process::ExitCode;

use compass_collectors::{
    balance_sheet, block_trade, cash_flow, dragon, fin_indicators, income, institution_survey,
    main_flow, stock_basic,
};

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

struct FinancialCliArgs {
    years: Option<Vec<i32>>,
    periods: String,
    page_size: usize,
    incremental: bool,
}

fn parse_financial_args(args: &[String]) -> Result<FinancialCliArgs, Box<dyn std::error::Error>> {
    let mut years: Option<Vec<i32>> = None;
    let mut periods = "Q1,Q2,Q3,FY".to_string();
    let mut page_size = 100usize;
    let mut incremental = false;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--years" => {
                let raw = args.get(i + 1).ok_or("--years requires a value")?;
                years = Some(
                    raw.split(',')
                        .filter_map(|s| s.trim().parse::<i32>().ok())
                        .collect(),
                );
                i += 2;
            }
            "--periods" => {
                periods = args
                    .get(i + 1)
                    .ok_or("--periods requires a value")?
                    .to_string();
                i += 2;
            }
            "--page-size" => {
                let raw = args.get(i + 1).ok_or("--page-size requires a value")?;
                page_size = raw.parse()?;
                i += 2;
            }
            "--incremental" => {
                incremental = true;
                i += 1;
            }
            other => return Err(format!("unknown flag {other}").into()),
        }
    }
    Ok(FinancialCliArgs {
        years,
        periods,
        page_size,
        incremental,
    })
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
        "dragon" => {
            let mut start = None;
            let mut end = None;
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
                    "--page-size" => {
                        let raw = args.get(i + 1).ok_or("--page-size requires a value")?;
                        page_size = raw.parse()?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let out = dragon::run(start, end, page_size).await?;
            println!("{}", out.display());
            Ok(())
        }
        "institution-survey" | "institution_survey" => {
            let mut start_date = None;
            let mut page_size = 100usize;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--start-date" => {
                        start_date = Some(
                            args.get(i + 1)
                                .ok_or("--start-date requires a value")?
                                .as_str(),
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
            let out = institution_survey::run(start_date, page_size).await?;
            println!("{}", out.display());
            Ok(())
        }
        "main-flow" | "main_flow" => {
            let mut page_size = 1000usize;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--page-size" => {
                        let raw = args.get(i + 1).ok_or("--page-size requires a value")?;
                        page_size = raw.parse()?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let out = main_flow::run(page_size).await?;
            println!("{}", out.display());
            Ok(())
        }
        "main-flow-backfill" | "main_flow_backfill" => {
            let mut start = None;
            let mut end = None;
            let mut symbols: Option<Vec<String>> = None;
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
                    "--symbols" => {
                        let raw = args.get(i + 1).ok_or("--symbols requires a value")?;
                        symbols = Some(
                            raw.split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect(),
                        );
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let start = start.ok_or("--start is required")?;
            let end = end.ok_or("--end is required")?;
            let out = main_flow::backfill(start, end, symbols.as_deref()).await?;
            println!("{}", out.display());
            Ok(())
        }
        "stock-basic" | "stock_basic" => {
            let mut output: Option<String> = None;
            let mut page_size = 100usize;
            let mut max_pages = stock_basic::MAX_PAGES;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--output" | "-o" => {
                        output = Some(
                            args.get(i + 1)
                                .ok_or("--output requires a value")?
                                .to_string(),
                        );
                        i += 2;
                    }
                    "--page-size" => {
                        let raw = args.get(i + 1).ok_or("--page-size requires a value")?;
                        page_size = raw.parse()?;
                        i += 2;
                    }
                    "--max-pages" => {
                        let raw = args.get(i + 1).ok_or("--max-pages requires a value")?;
                        max_pages = raw.parse()?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let out = stock_basic::run(output.as_deref(), page_size, max_pages).await?;
            println!("{}", out.display());
            Ok(())
        }
        "fin-indicators" | "fin_indicators" => {
            let args = parse_financial_args(args)?;
            let out = fin_indicators::run(
                args.years.as_deref(),
                &args.periods,
                args.page_size,
                args.incremental,
            )
            .await?;
            println!("{}", out.display());
            Ok(())
        }
        "balance-sheet" | "balance_sheet" => {
            let args = parse_financial_args(args)?;
            let out = balance_sheet::run(
                args.years.as_deref(),
                &args.periods,
                args.page_size,
                args.incremental,
            )
            .await?;
            println!("{}", out.display());
            Ok(())
        }
        "income" => {
            let args = parse_financial_args(args)?;
            let out = income::run(
                args.years.as_deref(),
                &args.periods,
                args.page_size,
                args.incremental,
            )
            .await?;
            println!("{}", out.display());
            Ok(())
        }
        "cash-flow" | "cash_flow" => {
            let args = parse_financial_args(args)?;
            let out = cash_flow::run(
                args.years.as_deref(),
                &args.periods,
                args.page_size,
                args.incremental,
            )
            .await?;
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
        "usage: compass-collectors <command> [options]\n\
         commands:\n\
         \x20 block-trade [--start D] [--end D] [--years Y,Y] [--page-size N]\n\
         \x20 dragon [--start D] [--end D] [--page-size N]\n\
         \x20 institution-survey [--start-date D] [--page-size N]\n\
         \x20 main-flow [--page-size N]\n\
         \x20 main-flow-backfill --start D --end D [--symbols S,S]\n\
         \x20 fin-indicators [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 balance-sheet [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 income [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 cash-flow [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 stock-basic [--output PATH] [--page-size N] [--max-pages N]"
    );
}
