use clap::{arg, Arg, ArgMatches, Command};

use crate::analysis_parameter::AnalysisParameter;

pub fn get_subcommand() -> Command {
    Command::new("growth")
        .about("Calculate growth curve from coverage histogram")
        .args(&[
            arg!(hist_file: <HIST_FILE> "Coverage histogram as tab-separated value (tsv) file"),
            // TODO: arg!(-a --hist "Also include histogram in output"),
            Arg::new("coverage").help("Ignore all countables with a coverage lower than the specified threshold. The coverage of a countable corresponds to the number of path/walk that contain it. Repeated appearances of a countable in the same path/walk are counted as one. You can pass a comma-separated list of coverage thresholds, each one will produce a separated growth curve (e.g., --coverage 2,3). Use --quorum to set a threshold in conjunction with each coverage (e.g., --quorum 0.5,0.9)")
            .short('l').long("coverage").default_value("1"),
            Arg::new("quorum").help("Unlike the --coverage parameter, which specifies a minimum constant number of paths for all growth point m (1 <= m <= num_paths), --quorum adjust the threshold based on m. At each m, a countable is counted in the average growth if the countable is contained in at least floor(m*quorum) paths. Example: A quorum of 0.9 requires a countable to be in 90% of paths for each subset size m. At m=10, it must appear in at least 9 paths. At m=100, it must appear in at least 90 paths. A quorum of 1 (100%) requires presence in all paths of the subset, corresponding to the core. Default: 0, a countable counts if it is present in any path at each growth point. Specify multiple quorum values with a comma-separated list (e.g., --quorum 0.5,0.9). Use --coverage to set static path thresholds in conjunction with variable quorum percentages (e.g., --coverage 5,10).")
            .short('q').long("quorum").default_value("0"),
        ])
}

pub fn get_instructions(
    args: &ArgMatches,
) -> Option<Result<Vec<AnalysisParameter>, anyhow::Error>> {
    if let Some(args) = args.subcommand_matches("growth") {
        let hist = args.get_one::<String>("hist_file").expect("").to_owned();
        let coverage = args.get_one::<String>("coverage").cloned();
        let quorum = args.get_one::<String>("quorum").cloned();
        Some(Ok(vec![AnalysisParameter::Growth {
            name: None,
            hist,
            coverage,
            quorum,
            display: true,
        }]))
    } else {
        None
    }
}
