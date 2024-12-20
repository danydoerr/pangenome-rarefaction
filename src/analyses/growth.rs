use core::{panic, str};
use std::path::Path;
use std::{collections::HashSet, fs, io::BufReader};

use rayon::iter::{IntoParallelRefIterator, ParallelBridge, ParallelIterator};

use crate::analysis_parameter::AnalysisParameter;
use crate::graph_broker::{GraphBroker, Hist, ThresholdContainer};
use crate::html_report::ReportItem;
use crate::{
    io::{parse_hists, write_table},
    util::CountType,
};

use super::{Analysis, AnalysisSection, ConstructibleAnalysis, InputRequirement};

type Hists = Vec<Hist>;
type Growths = Vec<(CountType, Vec<Vec<f64>>)>;
type Comments = Vec<Vec<u8>>;

pub struct Growth {
    parameter: AnalysisParameter,
    inner: Option<InnerGrowth>,
}

impl Analysis for Growth {
    fn get_type(&self) -> String {
        "Growth".to_string()
    }
    fn generate_table(
        &mut self,
        dm: Option<&crate::graph_broker::GraphBroker>,
    ) -> anyhow::Result<String> {
        log::info!("reporting hist table");

        self.set_inner(dm)?;
        let growths = &self.inner.as_ref().unwrap().growths;
        let hist_aux = &self.inner.as_ref().unwrap().hist_aux;
        let comments = &self.inner.as_ref().unwrap().comments;
        let mut res = String::new();
        for c in comments {
            res.push_str(str::from_utf8(&c[..])?);
            res.push_str("\n");
        }
        res.push_str(&format!(
            "# {}",
            std::env::args().collect::<Vec<String>>().join(" ")
        ));

        let mut header_cols = vec![vec![
            "panacus".to_string(),
            "count".to_string(),
            "coverage".to_string(),
            "quorum".to_string(),
        ]];
        let mut output_columns: Vec<Vec<f64>> = Vec::new();

        let hists = match &self.inner.as_ref().unwrap().hists {
            Some(h) => h.iter().collect::<Vec<_>>(),
            None => dm
                .expect("Growth needs either hist file or graph")
                .get_hists()
                .values()
                .collect::<Vec<_>>(),
        };

        for h in hists {
            output_columns.push(h.coverage.iter().map(|x| *x as f64).collect());
            header_cols.push(vec![
                "hist".to_string(),
                h.count.to_string(),
                String::new(),
                String::new(),
            ])
        }

        for (count, g) in growths {
            output_columns.extend(g.clone());
            let m = hist_aux.coverage.len();
            header_cols.extend(
                std::iter::repeat("growth")
                    .take(m)
                    .zip(std::iter::repeat(count).take(m))
                    .zip(hist_aux.coverage.iter())
                    .zip(&hist_aux.quorum)
                    .map(|(((p, t), c), q)| {
                        vec![p.to_string(), t.to_string(), c.get_string(), q.get_string()]
                    }),
            );
        }
        res.push_str(&write_table(&header_cols, &output_columns)?);
        Ok(res)
    }

    fn generate_report_section(
        &mut self,
        dm: Option<&crate::graph_broker::GraphBroker>,
    ) -> anyhow::Result<Vec<AnalysisSection>> {
        self.set_inner(dm)?;
        let hist_aux = &self.inner.as_ref().unwrap().hist_aux;
        let growth_labels = (0..hist_aux.coverage.len())
            .map(|i| {
                format!(
                    "coverage ≥ {}, quorum ≥ {}%",
                    hist_aux.coverage[i].get_string(),
                    hist_aux.quorum[i].get_string()
                )
            })
            .collect::<Vec<_>>();
        let table = self.generate_table(dm)?;
        let table = format!("`{}`", &table);
        let growths = &self.inner.as_ref().unwrap().growths;
        let id_prefix = format!(
            "pan-growth-{}",
            self.get_run_name()
                .to_lowercase()
                .replace(&[' ', '|', '\\'], "-")
        );
        let growth_tabs = growths
            .iter()
            .map(|(k, v)| AnalysisSection {
                id: format!("{id_prefix}-{k}"),
                analysis: "Pangenome Growth".to_string(),
                run_name: self.get_run_name(),
                countable: k.to_string(),
                table: Some(table.clone()),
                items: vec![ReportItem::MultiBar {
                    id: format!("{id_prefix}-{k}"),
                    names: growth_labels.clone(),
                    x_label: "taxa".to_string(),
                    y_label: format!("#{}s", k),
                    labels: (1..v[0].len()).map(|i| i.to_string()).collect(),
                    values: v.clone(),
                    log_toggle: false,
                }],
            })
            .collect();
        Ok(growth_tabs)
    }

    // fn get_subcommand() -> Command {
    //     Command::new("growth")
    //         .about("Calculate growth curve from coverage histogram")
    //         .args(&[
    //             arg!(hist_file: <HIST_FILE> "Coverage histogram as tab-separated value (tsv) file"),
    //             arg!(-a --hist "Also include histogram in output"),
    //             Arg::new("coverage").help("Ignore all countables with a coverage lower than the specified threshold. The coverage of a countable corresponds to the number of path/walk that contain it. Repeated appearances of a countable in the same path/walk are counted as one. You can pass a comma-separated list of coverage thresholds, each one will produce a separated growth curve (e.g., --coverage 2,3). Use --quorum to set a threshold in conjunction with each coverage (e.g., --quorum 0.5,0.9)")
    //                 .short('l').long("coverage").default_value("1"),
    //             Arg::new("quorum").help("Unlike the --coverage parameter, which specifies a minimum constant number of paths for all growth point m (1 <= m <= num_paths), --quorum adjust the threshold based on m. At each m, a countable is counted in the average growth if the countable is contained in at least floor(m*quorum) paths. Example: A quorum of 0.9 requires a countable to be in 90% of paths for each subset size m. At m=10, it must appear in at least 9 paths. At m=100, it must appear in at least 90 paths. A quorum of 1 (100%) requires presence in all paths of the subset, corresponding to the core. Default: 0, a countable counts if it is present in any path at each growth point. Specify multiple quorum values with a comma-separated list (e.g., --quorum 0.5,0.9). Use --coverage to set static path thresholds in conjunction with variable quorum percentages (e.g., --coverage 5,10).")
    //                 .short('q').long("quorum").default_value("0"),
    //         ])
    // }

    fn get_graph_requirements(&self) -> HashSet<super::InputRequirement> {
        if let AnalysisParameter::Growth { hist, .. } = &self.parameter {
            if Path::new(&hist).exists() {
                HashSet::new()
            } else {
                HashSet::from([InputRequirement::Hist])
            }
        } else {
            panic!("Growth should always contain growth parameter")
        }
    }
}

impl ConstructibleAnalysis for Growth {
    fn from_parameter(parameter: AnalysisParameter) -> Self {
        Growth {
            parameter,
            inner: None,
        }
    }
}

impl Growth {
    fn get_run_name(&self) -> String {
        match &self.parameter {
            AnalysisParameter::Growth { name, hist, .. } => {
                if name.is_some() {
                    return name.as_ref().unwrap().to_string();
                }
                format!("for hist {}", hist)
            }
            _ => panic!("Hist analysis needs to contain hist parameter"),
        }
    }

    fn set_inner(&mut self, gb: Option<&GraphBroker>) -> anyhow::Result<()> {
        if self.inner.is_some() {
            return Ok(());
        }
        if let AnalysisParameter::Growth {
            coverage,
            quorum,
            hist,
            ..
        } = &self.parameter
        {
            let quorum = quorum.to_owned().unwrap_or("0".to_string());
            let coverage = coverage.to_owned().unwrap_or("1".to_string());
            let hist_aux = ThresholdContainer::parse_params(&quorum, &coverage)?;

            if gb.is_none() {
                let hist_file = hist;
                log::info!("loading coverage histogram from {}", hist_file);
                let mut data = BufReader::new(fs::File::open(&hist_file)?);
                let (coverages, comments) = parse_hists(&mut data)?;
                let hists: Hists = coverages
                    .into_iter()
                    .map(|(count, coverage)| Hist { count, coverage })
                    .collect();
                let growths: Growths = hists
                    .par_iter()
                    .map(|h| (h.count, h.calc_all_growths(&hist_aux)))
                    .collect();
                self.inner = Some(InnerGrowth {
                    growths,
                    comments,
                    hist_aux,
                    hists: Some(hists),
                });
            } else {
                let gb = gb.unwrap();
                let growths: Growths = gb
                    .get_hists()
                    .values()
                    .par_bridge()
                    .map(|h| (h.count, h.calc_all_growths(&hist_aux)))
                    .collect();
                self.inner = Some(InnerGrowth {
                    growths,
                    comments: Vec::new(),
                    hist_aux,
                    hists: None,
                });
            }
            Ok(())
        } else {
            panic!("Growth should always contain growth parameter")
        }
    }
}

struct InnerGrowth {
    growths: Growths,
    comments: Comments,
    hist_aux: ThresholdContainer,
    hists: Option<Hists>,
}
