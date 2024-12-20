use std::io::Write;
use std::{
    collections::HashSet,
    io::{BufWriter, Error},
};

use clap::{arg, Arg, ArgMatches, Command};

use crate::clap_enum_variants;
use crate::{analyses::InputRequirement, graph_broker::GraphMaskParameters, util::CountType};

use super::{Analysis, AnalysisSection};

pub struct Table {
    total: bool,
}

impl Analysis for Table {
    fn build(
        _dm: &crate::graph_broker::GraphBroker,
        matches: &ArgMatches,
    ) -> Result<Box<Self>, Error> {
        let matches = matches.subcommand_matches("table").unwrap();
        Ok(Box::new(Self {
            total: matches.get_flag("total"),
        }))
    }

    fn write_table<W: Write>(
        &mut self,
        gb: &crate::graph_broker::GraphBroker,
        out: &mut BufWriter<W>,
    ) -> Result<(), Error> {
        log::info!("reporting coverage table");
        gb.write_abacus_by_group(self.total, out)
    }

    fn generate_report_section(
        &mut self,
        _dm: &crate::graph_broker::GraphBroker,
    ) -> Vec<AnalysisSection> {
        Vec::new()
    }

    fn get_subcommand() -> Command {
        Command::new("table")
            .about("Compute coverage table for count type")
            .args(&[
                arg!(gfa_file: <GFA_FILE> "graph in GFA1 format, accepts also compressed (.gz) file"),
                arg!(-s --subset <FILE> "Produce counts by subsetting the graph to a given list of paths (1-column list) or path coordinates (3- or 12-column BED file)"),
                arg!(-e --exclude <FILE> "Exclude bp/node/edge in growth count that intersect with paths (1-column list) or path coordinates (3- or 12-column BED-file) provided by the given file; all intersecting bp/node/edge will be exluded also in other paths not part of the given list"),
                arg!(-g --groupby <FILE> "Merge counts from paths by path-group mapping from given tab-separated two-column file"),
                arg!(-H --"groupby-haplotype" "Merge counts from paths belonging to same haplotype"),
                arg!(-S --"groupby-sample" "Merge counts from paths belonging to same sample"),
                arg!(-a --"total" "Summarize by totaling presence/absence over all groups"),
                Arg::new("count").help("Graph quantity to be counted").default_value("node").ignore_case(true).short('c').long("count").value_parser(clap_enum_variants!(CountType)),
            ])
    }

    fn get_input_requirements(
        matches: &clap::ArgMatches,
    ) -> Option<(
        HashSet<super::InputRequirement>,
        GraphMaskParameters,
        String,
    )> {
        let matches = matches.subcommand_matches("table")?;
        let mut req = HashSet::from([InputRequirement::Hist, InputRequirement::AbacusByGroup]);
        let count = matches.get_one::<CountType>("count").cloned().unwrap();
        req.extend(Self::count_to_input_req(count));
        let view = GraphMaskParameters {
            groupby: matches
                .get_one::<String>("groupby")
                .cloned()
                .unwrap_or_default(),
            groupby_haplotype: matches.get_flag("groupby-haplotype"),
            groupby_sample: matches.get_flag("groupby-sample"),
            positive_list: matches
                .get_one::<String>("subset")
                .cloned()
                .unwrap_or_default(),
            negative_list: matches
                .get_one::<String>("exclude")
                .cloned()
                .unwrap_or_default(),
            order: None,
        };
        let file_name = matches.get_one::<String>("gfa_file")?.to_owned();
        log::debug!("input params: {:?}, {:?}, {:?}", req, view, file_name);
        Some((req, view, file_name))
    }
}

impl Table {
    fn count_to_input_req(count: CountType) -> HashSet<InputRequirement> {
        match count {
            CountType::Bp => HashSet::from([InputRequirement::Bp]),
            CountType::Node => HashSet::from([InputRequirement::Node]),
            CountType::Edge => HashSet::from([InputRequirement::Edge]),
            CountType::All => HashSet::from([
                InputRequirement::Bp,
                InputRequirement::Node,
                InputRequirement::Edge,
            ]),
        }
    }
}
