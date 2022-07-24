/* standard use */
use std::fs;
use std::io::prelude::*;
use std::path::Path;
use std::str::FromStr;

/* crate use */
use clap::Parser;
use regex::Regex;
use rustc_hash::FxHashMap;

/* private use */
mod core;

#[derive(clap::Parser, Debug)]
#[clap(
    version = "0.2",
    author = "Daniel Doerr <daniel.doerr@hhu.de>",
    about = "Calculate growth statistics for pangenome graphs"
)]
pub struct Command {
    #[clap(index = 1, help = "graph in GFA1 format", required = true)]
    pub graph: String,

    #[clap(
        short = 't',
        long = "type",
        help = "type: node or edge count",
        default_value = "nodes",
        possible_values = &["nodes", "edges", "bp"],
    )]
    pub count_type: String,

    #[clap(
        short = 's',
        long = "subset",
        help = "produce counts by subsetting the graph to a given list of paths (1-column list) or path coordinates (3- or 12-column BED file)",
        default_value = ""
    )]
    pub positive_list: String,

    #[clap(
        short = 'e',
        long = "exclude",
        help = "exclude bps/nodes/edges in growth count that intersect with paths (1-column list) or path coordinates (3- or 12-column BED-file) provided by the given file",
        default_value = ""
    )]
    pub negative_list: String,

    #[clap(
        short = 'g',
        long = "groupby",
        help = "merge counts from paths by path-group mapping from given tab-separated two-column file",
        default_value = ""
    )]
    pub groups: String,

    #[clap(
        short = 'c',
        long = "coverage_thresholds",
        help = "list of (named) coverage thresholds of the form <threshold1>,<threshold2>,.. or <name1>=<threshold1>,<name2>=<threshold2> or a file that provides these thresholds in a tab-separated format; a threshold is absolute, i.e., corresponds to a number of paths/groups IFF it is integer, otherwise it is a float value representing a percentage of paths/groups.",
        default_value = "cumulative_count=1"
    )]
    pub thresholds: String,

    #[clap(
        short = 'a',
        long = "apriori",
        help = "identify coverage threshold groups a priori rather than during the cumulative counting"
    )]
    pub apriori: bool,

    #[clap(
        short = 'o',
        long = "ordered",
        help = "rather than computing growth across all permutations of the input, produce counts in the order of the paths in the GFA file, or, if a grouping file is specified, in the order of the provided groups"
    )]
    pub ordered: bool,
}

fn some_function<T>(abacus: core::Abacus<T>) {
    log::info!(
        "abacus has {} paths and {} countables",
        abacus.paths.len(),
        abacus.countable2path.len()
    );
}

fn main() -> Result<(), std::io::Error> {
    env_logger::init();

    // print output to stdout
    let mut out = std::io::BufWriter::new(std::io::stdout());

    // initialize command line parser & parse command line arguments
    let params = Command::parse();

    let mut subset_coords = Vec::new();
    if !params.positive_list.is_empty() {
        log::info!("loading subset coordinates from {}", &params.positive_list);
        let mut data = std::io::BufReader::new(fs::File::open(&params.positive_list)?);
        subset_coords = core::io::parse_bed(&mut data);
        log::debug!("loaded {} coordinates", subset_coords.len());
    }

    let mut exclude_coords = Vec::new();
    if !params.negative_list.is_empty() {
        log::info!(
            "loading exclusion coordinates from {}",
            &params.negative_list
        );
        let mut data = std::io::BufReader::new(fs::File::open(&params.negative_list)?);
        exclude_coords = core::io::parse_bed(&mut data);
        log::debug!("loaded {} coordinates", exclude_coords.len());
    }

    let mut groups = FxHashMap::default();
    if !params.groups.is_empty() {
        log::info!("loading groups from {}", &params.groups);
        let mut data = std::io::BufReader::new(fs::File::open(&params.groups)?);
        groups = core::io::parse_groups(&mut data);
        log::debug!("loaded {} group assignments ", groups.len());
    }

    let mut coverage_thresholds = Vec::new();
    if !params.thresholds.is_empty() {
        if Path::new(&params.thresholds).exists() {
            log::info!("loading coverage thresholds from {}", &params.thresholds);
            let mut data = std::io::BufReader::new(fs::File::open(&params.groups)?);
            coverage_thresholds = core::io::parse_coverage_threshold_file(&mut data);
        } else {
            let re = Regex::new(r"^\s?([!-<,>-~]+)\s?=\s?([!-<,>-~]+)\s*$").unwrap();
            for el in params.thresholds.split(',') {
                if let Some(t) = usize::from_str(el.trim()).ok() {
                    coverage_thresholds
                        .push((el.trim().to_string(), core::CoverageThreshold::Absolute(t)));
                } else if let Some(t) = f64::from_str(el.trim()).ok() {
                    coverage_thresholds
                        .push((el.trim().to_string(), core::CoverageThreshold::Relative(t)));
                } else if let Some(caps) = re.captures(&el) {
                    let name = caps.get(1).unwrap().as_str().trim().to_string();
                    let threshold_str = caps.get(2).unwrap().as_str();
                    let threshold = if let Some(t) = usize::from_str(threshold_str).ok() {
                        core::CoverageThreshold::Absolute(t)
                    } else {
                        core::CoverageThreshold::Relative(f64::from_str(threshold_str).unwrap())
                    };
                    coverage_thresholds.push((name, threshold));
                } else {
                    panic!(
                        "coverage threshold \"{}\" string is not well-formed",
                        &params.thresholds
                    );
                }
            }
        }
        log::debug!(
            "loaded {} coverage thresholds:\n{}",
            coverage_thresholds.len(),
            coverage_thresholds
                .iter()
                .map(|(n, t)| format!("\t{}: {}", n, t))
                .collect::<Vec<String>>()
                .join("\n")
        );
    }

    let mut walks_paths = 0;
    log::info!("first pass through file: counting P/W lines..");
    {
        let mut predata = std::io::BufReader::new(fs::File::open(&params.graph)?);
        walks_paths = core::io::count_pw_lines(&mut predata);
    }
    log::info!("..done; found {} paths/walks", &walks_paths);

    let mut data = std::io::BufReader::new(fs::File::open(&params.graph)?);

    log::info!("loading graph from {}", params.graph);
    let abacus = core::Abacus::<core::Node>::from_gfa(&mut data);

    some_function(abacus);

    out.flush()?;
    log::info!("done");
    Ok(())
}
