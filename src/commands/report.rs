use std::fs::File;
use std::io::BufReader;

use clap::{Arg, ArgAction, ArgMatches, Command};

use crate::analysis_parameter::AnalysisParameter;

pub fn get_subcommand() -> Command {
    Command::new("report")
        .about("Create an html report from a YAML config file")
        .args(&[Arg::new("yaml_file")
            .required(false)
            .help("Specifies yaml config")])
        .args(&[Arg::new("dry_run")
            .required(false)
            .long("dry-run")
            .short('d')
            .action(ArgAction::SetTrue)
            .help(
                "If set, no actual computation is done, only the planned computation will be shown",
            )])
}

pub fn get_instructions(
    args: &ArgMatches,
) -> Option<Result<Vec<AnalysisParameter>, anyhow::Error>> {
    if let Some(args) = args.subcommand_matches("report") {
        Some(parse_report_args(args))
    } else {
        None
    }
}

fn parse_report_args(args: &ArgMatches) -> Result<Vec<AnalysisParameter>, anyhow::Error> {
    if let Some(yaml_file) = args.get_one::<String>("yaml_file").cloned() {
        let f = File::open(yaml_file)?;
        let reader = BufReader::new(f);
        let contents = serde_yaml::from_reader(reader)?;
        Ok(contents)
    } else {
        println!(
            "
# Example yaml:
# To get started copy this into a .yaml file and edit it

- !Hist
  graph: ../simple_files/simple_graphs/t_groups.gfa
- !Hist
  name: testing this
  count_type: Bp
  graph: ../simple_files/simple_graphs/t_group2.gfa
  display: false
                "
        );
        Ok(Vec::new())
    }
}
