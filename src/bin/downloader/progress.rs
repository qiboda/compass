use indicatif::{MultiProgress, ProgressBar, ProgressStyle};

/// Visual progress tracker for the CLI download pipeline.
///
/// Uses [`indicatif::MultiProgress`] to coordinate two display elements:
/// * A **spinner** for ongoing activity notifications (e.g. "Enumerating symbols…").
/// * A **bar** that tracks overall symbol download progress.
pub struct DownloadProgress {
    #[allow(dead_code)]
    mp: MultiProgress,
    spinner: ProgressBar,
    bar: ProgressBar,
}

impl DownloadProgress {
    /// Create a new progress display with `total_symbols` as the bar maximum.
    ///
    /// The spinner starts immediately; the bar initialises at 0 / `total_symbols`.
    pub fn new(total_symbols: u64) -> Self {
        let mp = MultiProgress::new();

        let spinner = ProgressBar::new_spinner().with_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .expect("valid spinner template")
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        let spinner = mp.add(spinner);

        let bar = ProgressBar::new(total_symbols).with_style(
            ProgressStyle::with_template("{bar:40.cyan/blue} {pos:>4}/{len:4} {msg}")
                .expect("valid bar template")
                .progress_chars("##-"),
        );
        let bar = mp.add(bar);

        Self { mp, spinner, bar }
    }

    /// Update the spinner's accompanying text (e.g. current symbol name).
    pub fn set_spinner_message(&self, msg: &str) {
        self.spinner.set_message(msg.to_string());
    }

    /// Increment the symbol counter bar by one and update its message to
    /// show which symbol was just completed.
    pub fn inc_symbol(&self, symbol: &str) {
        self.bar.inc(1);
        self.bar.set_message(format!("completed {symbol}"));
    }

    /// Finalize both progress indicators (spinner tick → done, bar → done).
    pub fn finish(&self) {
        self.spinner.finish_with_message("done");
        self.bar.finish();
    }
}
