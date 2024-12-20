use core::panic;
use std::collections::HashSet;

use crate::analysis_parameter::AnalysisParameter;
use crate::html_report::ReportItem;
use crate::{analyses::InputRequirement, io::write_table, util::CountType};

use super::{Analysis, AnalysisSection, ConstructibleAnalysis};

pub struct Hist {
    parameter: AnalysisParameter,
}

impl Analysis for Hist {
    fn get_type(&self) -> String {
        "Hist".to_string()
    }

    fn generate_table(
        &mut self,
        gb: Option<&crate::graph_broker::GraphBroker>,
    ) -> anyhow::Result<String> {
        log::info!("reporting hist table");
        if gb.is_none() {
            panic!("Hist analysis needs a graph")
        }
        let gb = gb.unwrap();
        let mut res = String::new();
        res.push_str(&format!(
            "# {}\n",
            std::env::args().collect::<Vec<String>>().join(" ")
        ));

        let mut header_cols = vec![vec![
            "panacus".to_string(),
            "count".to_string(),
            String::new(),
            String::new(),
        ]];
        let mut output_columns = Vec::new();
        for h in gb.get_hists().values() {
            output_columns.push(h.coverage.iter().map(|x| *x as f64).collect());
            header_cols.push(vec![
                "hist".to_string(),
                h.count.to_string(),
                String::new(),
                String::new(),
            ])
        }
        res.push_str(&write_table(&header_cols, &output_columns)?);
        Ok(res)
    }

    fn generate_report_section(
        &mut self,
        gb: Option<&crate::graph_broker::GraphBroker>,
    ) -> anyhow::Result<Vec<AnalysisSection>> {
        if gb.is_none() {
            panic!("Hist analysis needs a graph")
        }
        let gb = gb.unwrap();
        let table = self.generate_table(Some(gb))?;
        let table = format!("`{}`", &table);
        let id_prefix = format!(
            "cov-hist-{}",
            self.get_run_name()
                .to_lowercase()
                .replace(&[' ', '|', '\\'], "-")
        );
        let histogram_tabs = gb
            .get_hists()
            .iter()
            .map(|(k, v)| AnalysisSection {
                id: format!("{id_prefix}-{k}"),
                analysis: "Coverage Histogram".to_string(),
                table: Some(table.clone()),
                run_name: self.get_run_name(),
                countable: k.to_string(),
                items: vec![ReportItem::Bar {
                    id: format!("{id_prefix}-{k}"),
                    name: gb.get_fname(),
                    x_label: "taxa".to_string(),
                    y_label: format!("#{}s", k),
                    labels: (0..v.coverage.len()).map(|s| s.to_string()).collect(),
                    values: v.coverage.iter().map(|c| *c as f64).collect(),
                    log_toggle: true,
                }],
            })
            .collect::<Vec<_>>();
        Ok(histogram_tabs)
    }

    fn get_graph_requirements(&self) -> HashSet<super::InputRequirement> {
        if let AnalysisParameter::Hist { count_type, .. } = &self.parameter {
            let mut req = HashSet::from([InputRequirement::Hist]);
            req.extend(Self::count_to_input_req(*count_type));
            // if let Some(subset) = subset {
            //     req.insert(InputRequirement::Subset(subset.to_owned()));
            // }
            // if let Some(grouping) = grouping {
            //     req.insert(InputRequirement::Grouping(grouping.to_owned()));
            // }
            // if let Some(exclude) = exclude {
            //     req.insert(InputRequirement::Exclude(exclude.to_owned()));
            // }
            req
        } else {
            HashSet::new()
        }
    }
}

impl ConstructibleAnalysis for Hist {
    fn from_parameter(parameter: AnalysisParameter) -> Self {
        Self { parameter }
    }
}

impl Hist {
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

    fn get_run_name(&self) -> String {
        match &self.parameter {
            AnalysisParameter::Hist {
                name,
                graph,
                subset,
                exclude,
                grouping,
                ..
            } => {
                if name.is_some() {
                    return name.as_ref().unwrap().to_string();
                }
                format!(
                    "{}-{}|{}\\{}",
                    graph,
                    match grouping.clone() {
                        Some(g) => g.to_string(),
                        None => "Ungrouped".to_string(),
                    },
                    subset.clone().unwrap_or_default(),
                    exclude.clone().unwrap_or_default()
                )
            }
            _ => panic!("Hist analysis needs to contain hist parameter"),
        }
    }
}
