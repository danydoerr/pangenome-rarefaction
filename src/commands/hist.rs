use crate::clap_enum_variants;
use clap::{arg, Arg, ArgMatches, Command};

use crate::analysis_parameter::{AnalysisParameter, Grouping};
use crate::util::CountType;

pub fn get_subcommand() -> Command {
    Command::new("hist")
        .about("Return general graph and paths info")
        .args(&[
            arg!(gfa_file: <GFA_FILE> "graph in GFA1 format, accepts also compressed (.gz) file"),
            arg!(-s --subset <FILE> "Produce counts by subsetting the graph to a given list of paths (1-column list) or path coordinates (3- or 12-column BED file)"),
            arg!(-e --exclude <FILE> "Exclude bp/node/edge in growth count that intersect with paths (1-column list) or path coordinates (3- or 12-column BED-file) provided by the given file; all intersecting bp/node/edge will be exluded also in other paths not part of the given list"),
            arg!(-g --groupby <FILE> "Merge counts from paths by path-group mapping from given tab-separated two-column file"),
            arg!(-H --"groupby-haplotype" "Merge counts from paths belonging to same haplotype"),
            arg!(-S --"groupby-sample" "Merge counts from paths belonging to same sample"),
            Arg::new("count").help("Graph quantity to be counted").default_value("node").ignore_case(true).short('c').long("count").value_parser(clap_enum_variants!(CountType)),
        ])
}

pub fn get_instructions(
    args: &ArgMatches,
) -> Option<Result<Vec<AnalysisParameter>, anyhow::Error>> {
    if let Some(args) = args.subcommand_matches("hist") {
        let graph = args
            .get_one::<String>("gfa_file")
            .expect("hist subcommand has gfa file")
            .to_owned();
        let count = args
            .get_one::<CountType>("count")
            .expect("hist subcommand has count type")
            .to_owned();
        let subset = args.get_one::<String>("subset").cloned();
        let exclude = args.get_one::<String>("exclude").cloned();
        let grouping = args.get_one::<String>("groupby").cloned();
        let grouping = if args.get_flag("groupby-sample") {
            Some(Grouping::Sample)
        } else if args.get_flag("groupby-haplotype") {
            Some(Grouping::Haplotype)
        } else {
            grouping.map(|g| Grouping::Custom(g))
        };
        Some(Ok(vec![AnalysisParameter::Hist {
            name: None,
            count_type: count,
            graph,
            display: true,
            subset,
            exclude,
            grouping,
        }]))
    } else {
        None
    }
}
