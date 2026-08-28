use std::process::ExitCode;

use compass_collectors::{
    balance_sheet, block_trade, cash_flow, check_proxy_pool, dragon, fin_indicators, freeproxy,
    income, index_daily, institution_survey, keepalive, main_flow, orchestrate, stock_basic,
    stock_basic_official,
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
    if let Some(ref years) = years
        && years.is_empty()
    {
        return Err("--years contains no valid years".into());
    }
    if page_size == 0 {
        return Err("--page-size must be greater than zero".into());
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
        "fetch" => {
            let target = args.get(1).ok_or("fetch requires a target")?;
            let mut years: Option<Vec<i32>> = None;
            let mut incremental = false;
            let mut i = 2;
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
                    "--incremental" => {
                        incremental = true;
                        i += 1;
                    }
                    other => return Err(format!("unknown fetch flag {other}").into()),
                }
            }
            if let Some(ref years) = years
                && years.is_empty()
            {
                return Err("--years contains no valid years".into());
            }
            let out = orchestrate::fetch(target, years.as_deref(), incremental).await?;
            println!("{}", out.display());
            Ok(())
        }
        "import" => {
            let target = args.get(1).ok_or("import requires a target")?;
            orchestrate::import_target(target).await?;
            Ok(())
        }
        "progress" => {
            let mut target: Option<String> = None;
            let mut as_json = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--json" => {
                        as_json = true;
                        i += 1;
                    }
                    other if !other.starts_with('-') => {
                        target = Some(other.to_string());
                        i += 1;
                    }
                    other => return Err(format!("unknown progress flag {other}").into()),
                }
            }
            orchestrate::progress(target.as_deref(), as_json).await?;
            Ok(())
        }
        "sync" => {
            orchestrate::sync(false).await?;
            Ok(())
        }
        "sync-investment" | "sync_investment" => {
            let mut restart = false;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--restart" => {
                        restart = true;
                        i += 1;
                    }
                    other => return Err(format!("unknown sync-investment flag {other}").into()),
                }
            }
            orchestrate::sync_investment(restart).await?;
            Ok(())
        }
        "backfill" => {
            let mut ranges: Vec<(String, (String, String))> = Vec::new();
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--table" => {
                        let table = args.get(i + 1).ok_or("--table requires a value")?;
                        let start = args.get(i + 2).ok_or("--start requires a value")?;
                        let end = args.get(i + 3).ok_or("--end requires a value")?;
                        ranges.push((table.to_string(), (start.to_string(), end.to_string())));
                        i += 4;
                    }
                    other => return Err(format!("unknown backfill flag {other}").into()),
                }
            }
            if ranges.is_empty() {
                return Err("backfill requires at least one --table T START END".into());
            }
            orchestrate::backfill(&ranges).await?;
            Ok(())
        }
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
        "index-daily" | "index_daily" => {
            let out = index_daily::run().await?;
            println!("{}", out.display());
            Ok(())
        }
        "index-daily-probe" | "index_daily_probe" => {
            let mut secid = None;
            let mut last_date = None;
            let mut output = None;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--secid" => {
                        secid = Some(args.get(i + 1).ok_or("--secid requires a value")?.as_str());
                        i += 2;
                    }
                    "--last-date" => {
                        last_date = Some(
                            args.get(i + 1)
                                .ok_or("--last-date requires a value")?
                                .as_str(),
                        );
                        i += 2;
                    }
                    "--output" | "-o" => {
                        output = Some(
                            args.get(i + 1)
                                .ok_or("--output requires a value")?
                                .to_string(),
                        );
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let secid = secid.ok_or("--secid is required")?;
            let output_path = std::path::PathBuf::from(
                output.unwrap_or_else(|| "index_daily_probe.csv".to_string()),
            );
            let (klines, _code) = index_daily::probe_official(secid, last_date).await?;
            std::fs::write(&output_path, klines.join("\n"))?;
            println!("{}", output_path.display());
            Ok(())
        }
        "index-daily-industries-probe" | "index_daily_industries_probe" => {
            let mut output = None;
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
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let output_path = std::path::PathBuf::from(
                output.unwrap_or_else(|| "index_daily_industries_probe.csv".to_string()),
            );
            let industries = index_daily::probe_ths_industries().await?;
            let body: Vec<String> = industries
                .iter()
                .map(|(code, name)| format!("{code},{name}"))
                .collect();
            std::fs::write(&output_path, body.join("\n"))?;
            println!("{}", output_path.display());
            Ok(())
        }
        "index-daily-backfill" | "index_daily_backfill" => {
            let mut start = None;
            let mut end = None;
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
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let start = start.ok_or("--start is required")?;
            let end = end.ok_or("--end is required")?;
            let out = index_daily::backfill(start, end).await?;
            println!("{}", out.display());
            Ok(())
        }
        "stock-basic-official" | "stock_basic_official" => {
            let mut output = None;
            let mut update_date = None;
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
                    "--update-date" => {
                        update_date = Some(
                            args.get(i + 1)
                                .ok_or("--update-date requires a value")?
                                .to_string(),
                        );
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            let out = stock_basic_official::run(output.as_deref(), update_date.as_deref()).await?;
            println!("{}", out.display());
            Ok(())
        }
        "freeproxy" => {
            let mut source = "json".to_string();
            let mut json_url = freeproxy::DEFAULT_JSON_URL.to_string();
            let mut redis_url = freeproxy::DEFAULT_REDIS_URL.to_string();
            let mut table = freeproxy::DEFAULT_TABLE.to_string();
            let mut limit = freeproxy::DEFAULT_LIMIT;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--source" => {
                        source = args
                            .get(i + 1)
                            .ok_or("--source requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--json-url" => {
                        json_url = args
                            .get(i + 1)
                            .ok_or("--json-url requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--redis-url" => {
                        redis_url = args
                            .get(i + 1)
                            .ok_or("--redis-url requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--table" => {
                        table = args
                            .get(i + 1)
                            .ok_or("--table requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or("--limit requires a value")?;
                        limit = raw.parse()?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            if source != "json" {
                return Err(
                    "freeproxy: --source realtime is not supported in Rust yet; use --source json"
                        .into(),
                );
            }
            let written = freeproxy::seed_json(&json_url, &redis_url, &table, limit).await?;
            println!("seeded {written} proxies into {table} ({source})");
            Ok(())
        }
        "keepalive" => {
            let mut once = false;
            let mut interval = 600u64;
            let mut json_url = freeproxy::DEFAULT_JSON_URL.to_string();
            let mut snapshot = keepalive::DEFAULT_SNAPSHOT.to_string();
            let mut redis_url = freeproxy::DEFAULT_REDIS_URL.to_string();
            let mut table = freeproxy::DEFAULT_TABLE.to_string();
            let mut limit = freeproxy::DEFAULT_LIMIT;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--once" => {
                        once = true;
                        i += 1;
                    }
                    "--interval" => {
                        let raw = args.get(i + 1).ok_or("--interval requires a value")?;
                        interval = raw.parse()?;
                        i += 2;
                    }
                    "--json-url" => {
                        json_url = args
                            .get(i + 1)
                            .ok_or("--json-url requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--snapshot" => {
                        snapshot = args
                            .get(i + 1)
                            .ok_or("--snapshot requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--redis-url" => {
                        redis_url = args
                            .get(i + 1)
                            .ok_or("--redis-url requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--table" => {
                        table = args
                            .get(i + 1)
                            .ok_or("--table requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--limit" => {
                        let raw = args.get(i + 1).ok_or("--limit requires a value")?;
                        limit = raw.parse()?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            if !once && interval == 0 {
                return Err("fatal: --interval must be > 0 unless --once is used".into());
            }
            let snapshot_path = std::path::PathBuf::from(snapshot);
            loop {
                let (json_written, realtime_written) =
                    keepalive::run_cycle(&json_url, &snapshot_path, &redis_url, &table, limit)
                        .await?;
                eprintln!(
                    "[keepalive] cycle done: json={json_written} realtime={realtime_written}"
                );
                if once {
                    return Ok(());
                }
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
            }
        }
        "check-proxy-pool" | "check_proxy_pool" => {
            let mut api_url = check_proxy_pool::DEFAULT_API_URL.to_string();
            let mut count = check_proxy_pool::DEFAULT_COUNT;
            let mut timeout = check_proxy_pool::DEFAULT_TIMEOUT;
            let mut i = 1;
            while i < args.len() {
                match args[i].as_str() {
                    "--api-url" => {
                        api_url = args
                            .get(i + 1)
                            .ok_or("--api-url requires a value")?
                            .to_string();
                        i += 2;
                    }
                    "--count" => {
                        let raw = args.get(i + 1).ok_or("--count requires a value")?;
                        count = raw.parse()?;
                        i += 2;
                    }
                    "--timeout" => {
                        let raw = args.get(i + 1).ok_or("--timeout requires a value")?;
                        timeout = raw.parse()?;
                        i += 2;
                    }
                    other => return Err(format!("unknown flag {other}").into()),
                }
            }
            if !timeout.is_finite() || timeout <= 0.0 {
                return Err("--timeout must be a positive finite number".into());
            }
            let payload = check_proxy_pool::run_with(&api_url, count, timeout).await?;
            println!("{}", serde_json::to_string_pretty(&payload)?);
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
         \x20 fetch <target> [--years Y,Y] [--incremental]\n\
         \x20 import <target>\n\
         \x20 sync\n\
         \x20 sync-investment [--restart]\n\
         \x20 progress [target] [--json]\n\
         \x20 block-trade [--start D] [--end D] [--years Y,Y] [--page-size N]\n\
         \x20 dragon [--start D] [--end D] [--page-size N]\n\
         \x20 institution-survey [--start-date D] [--page-size N]\n\
         \x20 main-flow [--page-size N]\n\
         \x20 main-flow-backfill --start D --end D [--symbols S,S]\n\
         \x20 fin-indicators [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 balance-sheet [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 income [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 cash-flow [--years Y,Y] [--periods Q1,Q2,FY] [--page-size N] [--incremental]\n\
         \x20 stock-basic [--output PATH] [--page-size N] [--max-pages N]\n\
         \x20 index-daily\n\
         \x20 index-daily-probe --secid ID [--last-date D] [--output PATH]\n\
         \x20 index-daily-industries-probe [--output PATH]\n\
         \x20 index-daily-backfill --start D --end D\n\
         \x20 stock-basic-official [--output PATH] [--update-date D]\n\
         \x20 freeproxy [--source json] [--json-url URL] [--redis-url URL] [--table T] [--limit N]\n\
         \x20 keepalive [--once] [--interval N] [--json-url URL] [--snapshot PATH]\n\
         \x20 check-proxy-pool"
    );
}
