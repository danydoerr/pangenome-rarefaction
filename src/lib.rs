/* private use */
pub mod analyses;
mod analysis_parameter;
mod commands;
pub mod graph_broker;
mod html_report;
mod io;
mod util;

use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    io::Write,
};
use thiserror::Error;

use analyses::{Analysis, ConstructibleAnalysis, InputRequirement};
use analysis_parameter::{AnalysisParameter, Grouping};
use clap::Command;
use graph_broker::GraphBroker;
use html_report::AnalysisSection;

#[macro_export]
macro_rules! clap_enum_variants {
    // Credit: Johan Andersson (https://github.com/repi)
    // Code from https://github.com/clap-rs/clap/discussions/4264
    ($e: ty) => {{
        use clap::builder::TypedValueParser;
        use strum::VariantNames;
        clap::builder::PossibleValuesParser::new(<$e>::VARIANTS).map(|s| s.parse::<$e>().unwrap())
    }};
}

#[macro_export]
macro_rules! clap_enum_variants_no_all {
    ($e: ty) => {{
        use clap::builder::TypedValueParser;
        clap::builder::PossibleValuesParser::new(<$e>::VARIANTS.iter().filter(|&x| x != &"all"))
            .map(|s| s.parse::<$e>().unwrap())
    }};
}

#[macro_export]
macro_rules! some_or_return {
    ($x:expr, $y:expr) => {
        match $x {
            Some(v) => v,
            None => return $y,
        }
    };
}

pub fn run_cli() -> Result<(), anyhow::Error> {
    let mut out = std::io::BufWriter::new(std::io::stdout());

    // read parameters and store them in memory
    // let params = cli::read_params();
    // cli::set_number_of_threads(&params);
    let args = Command::new("panacus")
        .subcommand(commands::report::get_subcommand())
        .subcommand(commands::hist::get_subcommand())
        .subcommand(commands::growth::get_subcommand())
        .subcommand(commands::histgrowth::get_subcommand())
        .subcommand(commands::info::get_subcommand())
        .subcommand_required(true)
        .get_matches();

    let mut instructions = Vec::new();
    let mut shall_write_html = false;
    let mut dry_run = false;
    if let Some(report) = commands::report::get_instructions(&args) {
        shall_write_html = true;
        instructions.extend(report?);
        if let Some(report_matches) = args.subcommand_matches("report") {
            dry_run = report_matches.get_flag("dry_run");
        }
    }
    if let Some(hist) = commands::hist::get_instructions(&args) {
        instructions.extend(hist?);
    }
    if let Some(growth) = commands::growth::get_instructions(&args) {
        instructions.extend(growth?);
    }
    if let Some(histgrowth) = commands::histgrowth::get_instructions(&args) {
        instructions.extend(histgrowth?);
    }
    if let Some(info) = commands::info::get_instructions(&args) {
        instructions.extend(info?);
    }

    let instructions = get_tasks(instructions)?;

    // ride on!
    if !dry_run {
        execute_pipeline(instructions, &mut out, shall_write_html)?;
    } else {
        println!("{:#?}", instructions);
    }

    // clean up & close down
    out.flush()?;
    Ok(())
}

#[derive(Error, Debug)]
pub enum ConfigParseError {
    #[error("no config block with name {name} was found")]
    NameNotFound { name: String },
}

fn get_tasks(instructions: Vec<AnalysisParameter>) -> anyhow::Result<Vec<Task>> {
    let instructions = preprocess_instructions(instructions)?;
    let mut tasks = Vec::new();
    let mut reqs = HashSet::new();
    let mut last_graph_change = 0usize;
    let mut current_subset = None;
    let mut current_exclude = String::new();
    let mut current_grouping = None;
    for instruction in instructions {
        match instruction {
            AnalysisParameter::Graph { nice, file, .. } => {
                tasks.push(Task::GraphChange(HashSet::new(), nice));
                if let Task::GraphChange(_, old_nice) = tasks[last_graph_change] {
                    tasks[last_graph_change] =
                        Task::GraphChange(std::mem::take(&mut reqs), old_nice);
                }
                reqs.insert(InputRequirement::Graph(file.to_string()));
                last_graph_change = tasks.len() - 1;
            }
            h @ AnalysisParameter::Hist { .. } => {
                if let AnalysisParameter::Hist {
                    subset,
                    exclude,
                    grouping,
                    ..
                } = &h
                {
                    let subset = subset.to_owned();
                    let exclude = exclude.clone().unwrap_or_default();
                    let grouping = grouping.to_owned();
                    if subset != current_subset {
                        tasks.push(Task::SubsetChange(subset.clone()));
                        current_subset = subset;
                    }
                    if exclude != current_exclude {
                        tasks.push(Task::ExcludeChange(exclude.clone()));
                        current_exclude = exclude;
                    }
                    if grouping != current_grouping {
                        tasks.push(Task::GroupingChange(grouping.clone()));
                        current_grouping = grouping;
                    }
                }
                let hist = analyses::hist::Hist::from_parameter(h);
                reqs.extend(hist.get_graph_requirements());
                tasks.push(Task::Analysis(Box::new(hist)));
            }
            g @ AnalysisParameter::Growth { .. } => {
                tasks.push(Task::Analysis(Box::new(
                    analyses::growth::Growth::from_parameter(g),
                )));
            }
            i @ AnalysisParameter::Info { .. } => {
                if let AnalysisParameter::Info {
                    subset,
                    exclude,
                    grouping,
                    ..
                } = &i
                {
                    let subset = subset.to_owned();
                    let exclude = exclude.clone().unwrap_or_default();
                    let grouping = grouping.to_owned();
                    if subset != current_subset {
                        tasks.push(Task::SubsetChange(subset.clone()));
                        current_subset = subset;
                    }
                    if exclude != current_exclude {
                        tasks.push(Task::ExcludeChange(exclude.clone()));
                        current_exclude = exclude;
                    }
                    if grouping != current_grouping {
                        tasks.push(Task::GroupingChange(grouping.clone()));
                        current_grouping = grouping;
                    }
                }
                let info = analyses::info::Info::from_parameter(i);
                reqs.extend(info.get_graph_requirements());
                tasks.push(Task::Analysis(Box::new(info)));
            }
            section @ _ => panic!(
                "YAML section {:?} should not exist after preprocessing",
                section
            ),
        }
    }
    if let Task::GraphChange(_, nice) = tasks[last_graph_change] {
        tasks[last_graph_change] = Task::GraphChange(reqs, nice);
    }
    Ok(tasks)
}

fn preprocess_instructions(
    instructions: Vec<AnalysisParameter>,
) -> anyhow::Result<Vec<AnalysisParameter>> {
    let graphs: HashMap<String, (String, bool)> = instructions
        .iter()
        .filter_map(|instruct| match instruct {
            AnalysisParameter::Graph { name, file, nice } => {
                Some((name.to_string(), (file.to_string(), *nice)))
            }
            _ => None,
        })
        .collect();
    let subsets: HashMap<String, String> = instructions
        .iter()
        .filter_map(|instruct| match instruct {
            AnalysisParameter::Subset { name, file } => Some((name.to_string(), file.to_string())),
            _ => None,
        })
        .collect();
    //let groupings: HashMap<String, String> = instructions
    //    .iter()
    //    .filter_map(|instruct| match instruct {
    //        AnalysisParameter::Grouping { name, file } => {
    //            Some((name.to_string(), file.to_string()))
    //        }
    //        _ => None,
    //    })
    //    .collect();
    let mut new_instructions: HashSet<AnalysisParameter> = HashSet::new();
    let mut counter = 0;
    let instructions = instructions
        .into_iter()
        .filter(|instruct| !matches!(instruct, AnalysisParameter::Subset { .. }))
        //.filter(|instruct| !matches!(instruct, AnalysisParameter::Grouping { .. }))
        .map(|instruct| match instruct {
            AnalysisParameter::Hist {
                graph,
                name,
                count_type,
                display,
                subset,
                exclude,
                grouping,
            } => {
                let subset = match subset {
                    Some(subset) => {
                        if subsets.contains_key(&subset) {
                            Some(subsets[&subset].to_string())
                        } else {
                            Some(subset)
                        }
                    }
                    None => None,
                };
                if !graphs.contains_key(&graph[..]) {
                    if !new_instructions
                        .iter()
                        .map(|i| match i {
                            AnalysisParameter::Graph { file, .. } if file.to_owned() == graph => {
                                true
                            }
                            _ => false,
                        })
                        .reduce(|acc, f| acc || f)
                        .unwrap_or(false)
                    {
                        counter += 1;
                        let new_name = format!("PANACUS_INTERNAL_GRAPH_{}", counter);
                        new_instructions.insert(AnalysisParameter::Graph {
                            name: new_name.clone(),
                            file: graph.clone(),
                            nice: false,
                        });
                    }
                    let new_name = format!("PANACUS_INTERNAL_GRAPH_{}", counter);
                    return AnalysisParameter::Hist {
                        name,
                        count_type,
                        graph: new_name,
                        display,
                        subset,
                        exclude,
                        grouping,
                    };
                }
                AnalysisParameter::Hist {
                    name,
                    count_type,
                    graph,
                    display,
                    subset,
                    exclude,
                    grouping,
                }
            }
            AnalysisParameter::Info {
                graph,
                subset,
                exclude,
                grouping,
            } => {
                let subset = match subset {
                    Some(subset) => {
                        if subsets.contains_key(&subset) {
                            Some(subsets[&subset].to_string())
                        } else {
                            Some(subset)
                        }
                    }
                    None => None,
                };
                if !graphs.contains_key(&graph[..]) {
                    if !new_instructions
                        .iter()
                        .map(|i| match i {
                            AnalysisParameter::Graph { file, .. } if file.to_owned() == graph => {
                                true
                            }
                            _ => false,
                        })
                        .reduce(|acc, f| acc || f)
                        .unwrap_or(false)
                    {
                        counter += 1;
                        let new_name = format!("PANACUS_INTERNAL_GRAPH_{}", counter);
                        new_instructions.insert(AnalysisParameter::Graph {
                            name: new_name.clone(),
                            file: graph.clone(),
                            nice: false,
                        });
                    }
                    let new_name = format!("PANACUS_INTERNAL_GRAPH_{}", counter);
                    return AnalysisParameter::Info {
                        graph: new_name,
                        subset,
                        exclude,
                        grouping,
                    };
                }
                AnalysisParameter::Info {
                    graph,
                    subset,
                    exclude,
                    grouping,
                }
            }
            p => p,
        })
        .collect();
    let mut instructions: Vec<AnalysisParameter> = instructions;
    instructions.extend(new_instructions.into_iter());
    let instructions = sort_instructions(instructions);
    let instructions = group_growths_to_hists(instructions)?;
    Ok(instructions)
}

fn sort_instructions(instructions: Vec<AnalysisParameter>) -> Vec<AnalysisParameter> {
    let (mut graph_statements, mut others): (Vec<_>, Vec<_>) = instructions
        .into_iter()
        .partition(|inst| matches!(inst, AnalysisParameter::Graph { .. }));
    graph_statements.sort();
    others.sort();
    // Needed so the insertion step can insert them always directly after
    // the graph section -> result is again sorted correctly
    others.reverse();
    let mut current_instructions = graph_statements;
    for instruction in others {
        match instruction {
            ref i @ AnalysisParameter::Info { ref graph, .. } => {
                insert_after_graph(i.clone(), graph, &mut current_instructions)
            }
            ref h @ AnalysisParameter::Hist { ref graph, .. } => {
                insert_after_graph(h.clone(), graph, &mut current_instructions)
            }
            o => current_instructions.insert(0, o),
        }
    }
    current_instructions
}

fn insert_after_graph(
    parameter: AnalysisParameter,
    graph: &str,
    instructions: &mut Vec<AnalysisParameter>,
) {
    for i in 0..instructions.len() {
        if let AnalysisParameter::Graph { name, .. } = &instructions[i] {
            if name == graph {
                instructions.insert(i + 1, parameter);
                return;
            }
        }
    }

    // TODO: is this necessary?
    // ensure that instruction is added
    instructions.push(parameter);
}

fn group_growths_to_hists(
    instructions: Vec<AnalysisParameter>,
) -> anyhow::Result<Vec<AnalysisParameter>> {
    let mut instructions = instructions;
    while has_ungrouped_growth(&instructions) {
        group_first_ungrouped_growth(&mut instructions)?;
    }
    Ok(instructions)
}

fn group_first_ungrouped_growth(instructions: &mut Vec<AnalysisParameter>) -> anyhow::Result<()> {
    let index_growth = instructions
        .iter()
        .position(|i| matches!(i, AnalysisParameter::Growth { .. }))
        .expect("Instructions need to have at least one growth");
    let hist_name = match &instructions[index_growth] {
        AnalysisParameter::Growth { hist, .. } => hist.to_string(),
        _ => panic!("index_growth should point to growth"),
    };
    let growth_instruction = instructions.remove(index_growth);
    let index_hist = instructions
        .iter()
        .position(
            |i| matches!(i, AnalysisParameter::Hist { name: Some(name), .. } if name == &hist_name),
        )
        .ok_or(ConfigParseError::NameNotFound {
            name: hist_name.clone(),
        })?;
    instructions.insert(index_hist + 1, growth_instruction);
    Ok(())
}

fn has_ungrouped_growth(instructions: &Vec<AnalysisParameter>) -> bool {
    for i in instructions {
        match i {
            AnalysisParameter::Growth { hist, .. } => {
                // Growth can only be ungrouped if it does not use a .tsv hist
                if !hist.ends_with(".tsv") {
                    return true;
                } else {
                    continue;
                }
            }
            AnalysisParameter::Hist { .. } => {
                return false;
            }
            _ => {
                continue;
            }
        }
    }
    false
}

pub enum Task {
    Analysis(Box<dyn Analysis>),
    GraphChange(HashSet<InputRequirement>, bool),
    SubsetChange(Option<String>),
    ExcludeChange(String),
    GroupingChange(Option<Grouping>),
}

impl Debug for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Analysis(analysis) => write!(f, "Analysis {}", analysis.get_type()),
            Self::GraphChange(reqs, nice) => f
                .debug_tuple("GraphChange")
                .field(&reqs)
                .field(nice)
                .finish(),
            Self::SubsetChange(subset) => f.debug_tuple("SubsetChange").field(&subset).finish(),
            Self::ExcludeChange(exclude) => f.debug_tuple("ExcludeChange").field(&exclude).finish(),
            Self::GroupingChange(grouping) => {
                f.debug_tuple("GroupingChange").field(&grouping).finish()
            }
        }
    }
}

pub fn execute_pipeline<W: Write>(
    mut instructions: Vec<Task>,
    out: &mut std::io::BufWriter<W>,
    shall_write_html: bool,
) -> anyhow::Result<()> {
    if instructions.is_empty() {
        log::warn!("No instructions supplied");
        return Ok(());
    }
    let mut report = Vec::new();
    let mut gb = match instructions[0] {
        _ => None,
    };
    for index in 0..instructions.len() {
        let is_next_analysis =
            instructions.len() > index + 1 && matches!(instructions[index + 1], Task::Analysis(..));
        match &mut instructions[index] {
            Task::Analysis(analysis) => {
                log::info!("Executing Analysis: {}", analysis.get_type());
                report.extend(analysis.generate_report_section(gb.as_ref())?);
            }
            Task::GraphChange(input_reqs, nice) => {
                log::info!("Executing graph change: {:?}", input_reqs);
                gb = Some(GraphBroker::from_gfa(&input_reqs, *nice));
                if is_next_analysis {
                    gb = Some(gb.expect("GraphBroker is some").finish()?);
                }
            }
            Task::SubsetChange(subset) => {
                log::info!("Executing subset change: {:?}", subset);
                gb = Some(
                    gb.expect("SubsetChange after Graph")
                        .include_coords(subset.as_ref().expect("Subset exists")),
                );
                if is_next_analysis {
                    gb = Some(gb.expect("GraphBroker is some").finish()?);
                }
            }
            Task::ExcludeChange(exclude) => {
                log::info!("Executing exclude change: {}", exclude);
                gb = Some(
                    gb.expect("ExcludeChange after Graph")
                        .exclude_coords(exclude),
                );
                if is_next_analysis {
                    gb = Some(gb.expect("GraphBroker is some").finish()?);
                }
            }
            Task::GroupingChange(grouping) => {
                log::info!("Executing grouping change: {:?}", grouping);
                gb = Some(gb.expect("GroupingChange after Graph").with_group(grouping));
                if is_next_analysis {
                    gb = Some(gb.expect("GraphBroker is some").finish()?);
                }
            }
        }
    }
    if shall_write_html {
        let mut registry = handlebars::Handlebars::new();
        let report =
            AnalysisSection::generate_report(report, &mut registry, "<Placeholder Filename>")?;
        writeln!(out, "{report}")?;
    } else {
        if let Task::Analysis(analysis) = instructions.last_mut().unwrap() {
            let table = analysis.generate_table(gb.as_ref())?;
            writeln!(out, "{table}")?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use analysis_parameter::Grouping;

    use super::*;

    fn get_graph_section(graph_name: &str) -> AnalysisParameter {
        AnalysisParameter::Graph {
            name: graph_name.to_string(),
            file: "./location/to/test_graph.gfa".to_string(),
            nice: false,
        }
    }

    fn get_hist_with_graph(graph: &str) -> AnalysisParameter {
        AnalysisParameter::Hist {
            name: None,
            count_type: util::CountType::Node,
            graph: graph.to_string(),
            display: false,
            subset: None,
            exclude: None,
            grouping: None,
        }
    }

    fn get_hist_with_subset(graph: &str, subset: &str) -> AnalysisParameter {
        AnalysisParameter::Hist {
            name: None,
            count_type: util::CountType::Node,
            graph: graph.to_string(),
            display: false,
            subset: Some(subset.to_string()),
            exclude: None,
            grouping: None,
        }
    }

    fn get_hist_with_exclude(graph: &str, exclude: &str) -> AnalysisParameter {
        AnalysisParameter::Hist {
            name: None,
            count_type: util::CountType::Node,
            graph: graph.to_string(),
            display: false,
            subset: None,
            exclude: Some(exclude.to_string()),
            grouping: None,
        }
    }

    fn get_hist_with_grouping(graph: &str, grouping: &str) -> AnalysisParameter {
        AnalysisParameter::Hist {
            name: None,
            count_type: util::CountType::Node,
            graph: graph.to_string(),
            display: false,
            subset: None,
            exclude: None,
            grouping: Some(Grouping::Custom(grouping.to_string())),
        }
    }

    fn get_hist_with_name(name: &str) -> AnalysisParameter {
        AnalysisParameter::Hist {
            name: Some(name.to_string()),
            count_type: util::CountType::Node,
            graph: "test_graph".to_string(),
            display: false,
            subset: None,
            exclude: None,
            grouping: None,
        }
    }

    fn get_growth_with_hist(hist: &str) -> AnalysisParameter {
        AnalysisParameter::Growth {
            name: None,
            coverage: None,
            quorum: None,
            hist: hist.to_string(),
            display: false,
        }
    }

    #[test]
    fn test_replace_graph_name() {
        let instructions = vec![get_hist_with_graph("./location/to/test_graph.gfa")];
        let expected = vec![
            get_graph_section("PANACUS_INTERNAL_GRAPH_1"),
            get_hist_with_graph("PANACUS_INTERNAL_GRAPH_1"),
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }

    #[test]
    fn test_replace_subset_name() {
        let instructions = vec![
            get_graph_section("test"),
            AnalysisParameter::Hist {
                name: None,
                count_type: util::CountType::Node,
                graph: "test".to_string(),
                display: false,
                subset: Some("test_subset".to_string()),
                exclude: None,
                grouping: None,
            },
            AnalysisParameter::Subset {
                name: "test_subset".to_string(),
                file: "subset_file.bed".to_string(),
            },
        ];
        let expected = vec![
            get_graph_section("test"),
            AnalysisParameter::Hist {
                name: None,
                count_type: util::CountType::Node,
                graph: "test".to_string(),
                display: false,
                subset: Some("subset_file.bed".to_string()),
                exclude: None,
                grouping: None,
            },
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }

    #[test]
    fn test_sort_hist_by_name() {
        let instructions = vec![
            get_graph_section("test_graph"),
            get_hist_with_name("B"),
            get_hist_with_name("Z"),
            get_hist_with_name("A"),
        ];
        let expected = vec![
            get_graph_section("test_graph"),
            get_hist_with_name("A"),
            get_hist_with_name("B"),
            get_hist_with_name("Z"),
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }

    #[test]
    fn test_sort_by_graph() {
        let instructions = vec![
            get_graph_section("B"),
            get_graph_section("A"),
            get_hist_with_graph("A"),
            get_hist_with_graph("B"),
            get_hist_with_graph("A"),
        ];
        let expected = vec![
            get_graph_section("A"),
            get_hist_with_graph("A"),
            get_hist_with_graph("A"),
            get_graph_section("B"),
            get_hist_with_graph("B"),
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }

    #[test]
    fn test_sort_by_subset() {
        let instructions = vec![
            get_graph_section("graph_a"),
            get_graph_section("graph_b"),
            get_hist_with_subset("graph_a", "subset_a"),
            get_hist_with_subset("graph_b", "subset_a"),
            get_hist_with_subset("graph_a", "subset_b"),
            get_hist_with_subset("graph_a", "subset_a"),
        ];
        let expected = vec![
            get_graph_section("graph_a"),
            get_hist_with_subset("graph_a", "subset_a"),
            get_hist_with_subset("graph_a", "subset_a"),
            get_hist_with_subset("graph_a", "subset_b"),
            get_graph_section("graph_b"),
            get_hist_with_subset("graph_b", "subset_a"),
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }

    #[test]
    fn test_sort_by_exclude() {
        let instructions = vec![
            get_graph_section("graph_a"),
            get_graph_section("graph_b"),
            get_hist_with_exclude("graph_a", "exclude_a"),
            get_hist_with_exclude("graph_b", "exclude_a"),
            get_hist_with_exclude("graph_a", "exclude_b"),
            get_hist_with_exclude("graph_a", "exclude_a"),
        ];
        let expected = vec![
            get_graph_section("graph_a"),
            get_hist_with_exclude("graph_a", "exclude_a"),
            get_hist_with_exclude("graph_a", "exclude_a"),
            get_hist_with_exclude("graph_a", "exclude_b"),
            get_graph_section("graph_b"),
            get_hist_with_exclude("graph_b", "exclude_a"),
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }

    #[test]
    fn test_sort_by_grouping() {
        let instructions = vec![
            get_graph_section("graph_a"),
            get_graph_section("graph_b"),
            get_hist_with_grouping("graph_a", "grouping_a"),
            get_hist_with_grouping("graph_b", "grouping_a"),
            get_hist_with_grouping("graph_a", "grouping_b"),
            get_hist_with_grouping("graph_a", "grouping_a"),
        ];
        let expected = vec![
            get_graph_section("graph_a"),
            get_hist_with_grouping("graph_a", "grouping_a"),
            get_hist_with_grouping("graph_a", "grouping_a"),
            get_hist_with_grouping("graph_a", "grouping_b"),
            get_graph_section("graph_b"),
            get_hist_with_grouping("graph_b", "grouping_a"),
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }

    #[test]
    fn test_group_growth_to_hist() {
        let instructions = vec![
            get_graph_section("test_graph"),
            get_growth_with_hist("B"),
            get_growth_with_hist("C"),
            get_growth_with_hist("A"),
            get_hist_with_name("C"),
            get_hist_with_name("B"),
            get_hist_with_name("A"),
        ];
        let expected = vec![
            get_graph_section("test_graph"),
            get_hist_with_name("A"),
            get_growth_with_hist("A"),
            get_hist_with_name("B"),
            get_growth_with_hist("B"),
            get_hist_with_name("C"),
            get_growth_with_hist("C"),
        ];
        let calculated = preprocess_instructions(instructions).unwrap();
        assert_eq!(calculated, expected);
    }
}
