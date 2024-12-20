use core::panic;
use std::fmt::Display;

use serde::{Deserialize, Serialize};

use crate::util::CountType;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone)]
pub enum AnalysisParameter {
    Hist {
        name: Option<String>,

        #[serde(default)]
        count_type: CountType,
        graph: String,

        #[serde(default = "get_true")]
        display: bool,

        subset: Option<String>,
        exclude: Option<String>,
        grouping: Option<Grouping>,
    },
    Growth {
        name: Option<String>,

        coverage: Option<String>,
        quorum: Option<String>,

        hist: String,

        #[serde(default = "get_true")]
        display: bool,
    },
    Subset {
        name: String,
        file: String,
    },
    Graph {
        name: String,
        file: String,
        #[serde(default)]
        nice: bool,
    },
    Info {
        graph: String,
        subset: Option<String>,
        exclude: Option<String>,
        grouping: Option<Grouping>,
    },
    OrderedGrowth,
    Table,
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Hash, Clone, PartialOrd, Ord)]
pub enum Grouping {
    Sample,
    Haplotype,
    Custom(String),
}

impl Display for Grouping {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sample => write!(f, "Group By Sample"),
            Self::Haplotype => write!(f, "Group By Haplotype"),
            Self::Custom(file) => write!(f, "Group By {}", file),
        }
    }
}

fn get_true() -> bool {
    true
}

impl AnalysisParameter {
    fn compare_hists(&self, other: &Self) -> std::cmp::Ordering {
        match self {
            AnalysisParameter::Hist {
                name,
                count_type,
                graph,
                display,
                subset,
                exclude,
                grouping,
            } => match other {
                AnalysisParameter::Hist {
                    name: o_name,
                    count_type: o_count_type,
                    graph: o_graph,
                    display: o_display,
                    subset: o_subset,
                    exclude: o_exclude,
                    grouping: o_grouping,
                } => {
                    if graph != o_graph {
                        return graph.cmp(o_graph);
                    } else if subset != o_subset {
                        return subset.cmp(o_subset);
                    } else if exclude != o_exclude {
                        return exclude.cmp(o_exclude);
                    } else if grouping != o_grouping {
                        return grouping.cmp(o_grouping);
                    } else if count_type != o_count_type {
                        return count_type.cmp(o_count_type);
                    } else if name != o_name {
                        return name.cmp(o_name);
                    } else if display != o_display {
                        return display.cmp(o_display);
                    } else {
                        return std::cmp::Ordering::Equal;
                    }
                }
                _ => panic!("Method only defined for hists"),
            },
            _ => panic!("Method only defined for hists"),
        }
    }
}

impl PartialOrd for AnalysisParameter {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for AnalysisParameter {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match self {
            AnalysisParameter::Hist { .. } => match other {
                o @ AnalysisParameter::Hist { .. } => self.compare_hists(o),
                _ => std::cmp::Ordering::Greater,
            },
            AnalysisParameter::Graph { name, .. } => match other {
                AnalysisParameter::Graph { name: o_name, .. } => name.cmp(o_name),
                _ => std::cmp::Ordering::Less,
            },
            AnalysisParameter::Subset { name, .. } => match other {
                AnalysisParameter::Subset { name: o_name, .. } => name.cmp(o_name),
                AnalysisParameter::Graph { .. } => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Less,
            },
            //AnalysisParameter::Grouping { name, .. } => match other {
            //    AnalysisParameter::Grouping { name: o_name, .. } => name.cmp(o_name),
            //    AnalysisParameter::Graph { .. } | AnalysisParameter::Subset { .. } => {
            //        std::cmp::Ordering::Greater
            //    }
            //    _ => std::cmp::Ordering::Less,
            //},
            AnalysisParameter::Info {
                graph,
                subset,
                exclude,
                grouping,
            } => match other {
                AnalysisParameter::Info { graph: o_graph, subset: o_subset, exclude: o_exclude, grouping: o_grouping } => {
                    graph.cmp(o_graph).then(subset.cmp(o_subset)).then(exclude.cmp(o_exclude)).then(grouping.cmp(o_grouping))
                },
                AnalysisParameter::Graph { .. }
                //| AnalysisParameter::Grouping { .. }
                | AnalysisParameter::Subset { .. } => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Less,
            },
            AnalysisParameter::Table => match other {
                AnalysisParameter::Table => std::cmp::Ordering::Equal,
                AnalysisParameter::Graph { .. }
                | AnalysisParameter::Subset { .. }
                //| AnalysisParameter::Grouping { .. }
                | AnalysisParameter::Info { .. } => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Less,
            },
            AnalysisParameter::OrderedGrowth => match other {
                AnalysisParameter::OrderedGrowth => std::cmp::Ordering::Equal,
                AnalysisParameter::Graph { .. }
                | AnalysisParameter::Subset { .. }
                //| AnalysisParameter::Grouping { .. }
                | AnalysisParameter::Info { .. }
                | AnalysisParameter::Table => std::cmp::Ordering::Greater,
                _ => std::cmp::Ordering::Less,
            },
            AnalysisParameter::Growth { hist, .. } => match other {
                AnalysisParameter::Growth { hist: o_hist, .. } => hist.cmp(o_hist),
                AnalysisParameter::Hist { .. } => std::cmp::Ordering::Less,
                _ => std::cmp::Ordering::Greater,
            },
        }
    }
}
