/* standard crate */
use std::io::{BufWriter, Write};
use std::io::{Error, ErrorKind};
use std::str::FromStr;

/* external crate */
use clap::{crate_version, value_parser, Arg, ArgMatches, Command};
use handlebars::Handlebars;

/* private use */
use crate::analyses::{self, Analysis};
use crate::graph_broker::GraphBroker;
use crate::html_report::AnalysisSection;
use crate::io::OutputFormat;
use crate::util::*;

pub enum RequireThreshold {
    Absolute,
    Relative,
    #[allow(dead_code)]
    Either,
}

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

pub fn parse_threshold_cli(
    threshold_str: &str,
    require: RequireThreshold,
) -> Result<Vec<Threshold>, Error> {
    let mut thresholds = Vec::new();

    for (i, el) in threshold_str.split(',').enumerate() {
        let rel_val = match f64::from_str(el.trim()) {
            Ok(t) => {
                if (0.0..=1.0).contains(&t) {
                    Ok(t)
                } else {
                    Err(Error::new(
                        ErrorKind::InvalidData,
                        format!(
                            "relative threshold \"{}\" ({}. element in list) must be within [0,1].",
                            &threshold_str,
                            i + 1
                        ),
                    ))
                }
            }
            Err(_) => Err(Error::new(
                ErrorKind::InvalidData,
                format!(
                    "threshold \"{}\" ({}. element in list) is required to be float, but isn't.",
                    &threshold_str,
                    i + 1
                ),
            )),
        };

        thresholds.push(
            match require {
                RequireThreshold::Absolute => Threshold::Absolute(usize::from_str(el.trim()).map_err(|_|
                    Error::new(
                            ErrorKind::InvalidData,
                            format!("threshold \"{}\" ({}. element in list) is required to be integer, but isn't.",
                    &threshold_str,
                    i + 1)))?),
            RequireThreshold::Relative => Threshold::Relative(rel_val?),
            RequireThreshold::Either =>
        if let Ok(t) = usize::from_str(el.trim()) {
            Threshold::Absolute(t)
        } else {
            Threshold::Relative(rel_val?)
            }
            }
            );
    }
    Ok(thresholds)
}

// set number of threads can be run only once, otherwise it throws an error of the
// GlobalPoolAlreadyInitialized, which unfortunately is not pub therefore we cannot catch it.
// https://github.com/rayon-rs/rayon/issues/878
// We run this function in the main otherwise in the tests the second time we run the function
// "run" it will crush
pub fn set_number_of_threads(params: &ArgMatches) {
    //if num_threads is 0 then the Rayon will select
    //the number of threads to the core number automatically
    let threads = params.get_one("threads").unwrap();
    rayon::ThreadPoolBuilder::new()
        .num_threads(*threads)
        .build_global()
        .expect("Failed to initialize global thread pool");
    log::info!(
        "running panacus on {} threads",
        rayon::current_num_threads()
    );
}

fn write_output<T: Analysis, W: Write>(
    mut analysis: T,
    gb: &GraphBroker,
    out: &mut BufWriter<W>,
    matches: &ArgMatches,
) -> Result<(), Error> {
    match matches.get_one("output_format").unwrap() {
        OutputFormat::Table => {
            analysis.write_table(gb, out)?;
        }
        OutputFormat::Html => {
            let report = analysis.generate_report_section(gb);
            let mut registry = Handlebars::new();
            let html =
                AnalysisSection::generate_report(report, &mut registry, &gb.get_fname()).unwrap();
            writeln!(out, "{}", html)?;
        }
    };
    Ok(())
}

pub fn run<W: Write>(out: &mut BufWriter<W>) -> Result<(), Error> {
    // let matches = Command::new("")
    //     .version(crate_version!())
    //     .author("Luca Parmigiani <lparmig@cebitec.uni-bielefeld.de>, Daniel Doerr <daniel.doerr@hhu.de>")
    //     .about("Calculate count statistics for pangenomic data")
    //     .args(&[
    //         Arg::new("output_format").global(true).help("Choose output format: table (tab-separated-values) or html report").short('o').long("output-format")
    //         .default_value("table").value_parser(clap_enum_variants!(OutputFormat)).ignore_case(true),
    //         Arg::new("threads").global(true).short('t').long("threads").help("").default_value("0").value_parser(value_parser!(usize)),
    //     ])
    //     .subcommand(Info::get_subcommand())
    //     .subcommand(analyses::hist::Hist::get_subcommand())
    //     .subcommand(Histgrowth::get_subcommand())
    //     .subcommand(OrderedHistgrowth::get_subcommand())
    //     .subcommand(Table::get_subcommand())
    //     .subcommand(Growth::get_subcommand())
    //     .get_matches();

    // set_number_of_threads(&matches);
    // if let Some((req, view_params, gfa_file)) = Info::get_input_requirements(&matches) {
    //     let gb = GraphBroker::from_gfa_with_view(&gfa_file, req, &view_params)?;
    //     let info = Info::build(&gb, &matches)?;
    //     write_output(*info, &gb, out, &matches)?;
    // } else if let Some((req, view_params, gfa_file)) =
    //     crate::analyses::hist::Hist::get_input_requirements(&matches)
    // {
    //     let gb = GraphBroker::from_gfa_with_view(&gfa_file, req, &view_params)?;
    //     let hist = analyses::hist::Hist::build(&gb, &matches)?;
    //     write_output(*hist, &gb, out, &matches)?;
    // } else if let Some((req, view_params, gfa_file)) = Histgrowth::get_input_requirements(&matches)
    // {
    //     let gb = GraphBroker::from_gfa_with_view(&gfa_file, req, &view_params)?;
    //     let histgrowth = Histgrowth::build(&gb, &matches)?;
    //     write_output(*histgrowth, &gb, out, &matches)?;
    // } else if let Some((req, view_params, gfa_file)) =
    //     OrderedHistgrowth::get_input_requirements(&matches)
    // {
    //     let gb = GraphBroker::from_gfa_with_view(&gfa_file, req, &view_params)?;
    //     let ordered_histgrowth = OrderedHistgrowth::build(&gb, &matches)?;
    //     write_output(*ordered_histgrowth, &gb, out, &matches)?;
    // } else if let Some((req, view_params, gfa_file)) = Table::get_input_requirements(&matches) {
    //     let gb = GraphBroker::from_gfa_with_view(&gfa_file, req, &view_params)?;
    //     let table = Table::build(&gb, &matches)?;
    //     write_output(*table, &gb, out, &matches)?;
    // } else if matches.subcommand_matches("growth").is_some() {
    //     let gb = GraphBroker::new();
    //     let growth = Growth::build(&gb, &matches)?;
    //     write_output(*growth, &gb, out, &matches)?;
    // }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_threshold_cli_relative_success() {
        let threshold_str = "0.2,0.5,0.9";
        let result = parse_threshold_cli(threshold_str, RequireThreshold::Relative);
        assert!(result.is_ok());
        let thresholds = result.unwrap();
        assert_eq!(thresholds.len(), 3);
        assert_eq!(thresholds[0], Threshold::Relative(0.2));
        assert_eq!(thresholds[1], Threshold::Relative(0.5));
        assert_eq!(thresholds[2], Threshold::Relative(0.9));
    }

    #[test]
    fn test_parse_threshold_cli_absolute_success() {
        let threshold_str = "5,10,15";
        let result = parse_threshold_cli(threshold_str, RequireThreshold::Absolute);
        assert!(result.is_ok());
        let thresholds = result.unwrap();
        assert_eq!(thresholds.len(), 3);
        assert_eq!(thresholds[0], Threshold::Absolute(5));
        assert_eq!(thresholds[1], Threshold::Absolute(10));
        assert_eq!(thresholds[2], Threshold::Absolute(15));
    }

    #[test]
    fn test_parse_threshold_cli_invalid_float_in_absolute() {
        let threshold_str = "5.5,10,15";
        let result = parse_threshold_cli(threshold_str, RequireThreshold::Absolute);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData);
    }

    #[test]
    fn test_parse_threshold_cli_invalid_value_in_relative() {
        let threshold_str = "0.2,1.2,0.9"; // 1.2 is out of range for relative threshold
        let result = parse_threshold_cli(threshold_str, RequireThreshold::Relative);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData);
    }
}
