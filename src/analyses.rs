pub mod growth;
pub mod hist;
// pub mod histgrowth;
pub mod info;
// pub mod ordered_histgrowth;
// pub mod table;

use std::collections::HashSet;

use crate::{
    analysis_parameter::AnalysisParameter, graph_broker::GraphBroker, html_report::AnalysisSection,
};

pub trait Analysis {
    fn generate_table(&mut self, gb: Option<&GraphBroker>) -> anyhow::Result<String>;
    fn generate_report_section(
        &mut self,
        gb: Option<&GraphBroker>,
    ) -> anyhow::Result<Vec<AnalysisSection>>;
    fn get_graph_requirements(&self) -> HashSet<InputRequirement>;
    fn get_type(&self) -> String;
}

pub trait ConstructibleAnalysis: Analysis {
    fn from_parameter(parameter: AnalysisParameter) -> Self;
}

#[derive(PartialEq, Eq, Debug, Clone, Hash)]
pub enum InputRequirement {
    Node,
    Edge,
    Bp,
    PathLens,
    Hist,
    AbacusByGroup,
    Graph(String),
    // Subset(String),
    // Grouping(String),
    // Exclude(String),
}
