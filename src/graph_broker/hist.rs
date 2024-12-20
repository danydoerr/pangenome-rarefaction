/* standard use */
use std::io::Write;
use std::io::{Error, ErrorKind};
use std::str::FromStr;

/* external crate */
use rayon::prelude::*;

/* private use */
use crate::util::{CountType, Threshold};

use super::abacus::AbacusByTotal;
use super::graph::GraphStorage;

#[derive(Debug, Clone)]
pub struct Hist {
    pub count: CountType,
    pub coverage: Vec<usize>,
}

pub fn choose(n: usize, k: usize) -> f64 {
    let mut res: f64 = 0.0;
    if k > n {
        return 0.0;
    }

    let k = if k > n - k { n - k } else { k };

    let n = n as f64;

    for i in 0..k {
        res += (n - i as f64).log2();
        res -= (i as f64 + 1.0).log2();
    }
    res
}

impl Hist {
    pub fn from_abacus(abacus: &AbacusByTotal, graph_aux: Option<&GraphStorage>) -> Self {
        Self {
            count: abacus.count,
            coverage: match abacus.count {
                CountType::Node | CountType::Edge => abacus.construct_hist(),
                CountType::Bp => abacus
                    .construct_hist_bps(graph_aux.expect("Graph auxiliary is needed for Bps hist")),
                CountType::All => unreachable!("inadmissable count type"),
            },
        }
    }

    pub fn calc_growth(&self, t_coverage: &Threshold, t_quorum: &Threshold) -> Vec<f64> {
        let n = self.coverage.len() - 1;

        if n > 0 {
            let quorum = usize::max(1, t_quorum.to_absolute(n));
            if quorum == 1 {
                self.calc_growth_union(t_coverage)
            } else if quorum >= n {
                self.calc_growth_core(t_coverage)
            } else {
                self.calc_growth_quorum(t_coverage, t_quorum)
            }
        } else {
            Vec::new()
        }
    }

    pub fn calc_all_growths(&self, hist_aux: &ThresholdContainer) -> Vec<Vec<f64>> {
        let mut growths: Vec<Vec<f64>> = hist_aux
            .coverage
            .par_iter()
            .zip(&hist_aux.quorum)
            .map(|(c, q)| {
                log::info!(
                    "calculating growth for coverage >= {} and quorum >= {}",
                    &c,
                    &q
                );
                self.calc_growth(c, q)
            })
            .collect();
        // insert empty row for 0 element
        for g in &mut growths {
            g.insert(0, f64::NAN);
        }
        growths
    }

    pub fn calc_growth_union(&self, t_coverage: &Threshold) -> Vec<f64> {
        let n = self.coverage.len() - 1; // hist array has length n+1: from 0..n (both included)
        let c = usize::max(1, t_coverage.to_absolute(n));

        let mut pangrowth: Vec<f64> = vec![0.0; n];
        let mut n_fall_m: f64 = 0.0;
        let tot = self.coverage[c..].iter().sum::<usize>() as f64;

        // perc_mult[i] contains the percentage of combinations that
        // have an item of multiplicity i
        let mut perc_mult: Vec<f64> = Vec::with_capacity(n + 1);
        perc_mult.resize(n + 1, 0.0);

        for m in 1..n + 1 {
            let mut y: f64 = 0.0;
            n_fall_m += (n as f64 - m as f64 + 1.0).log2();
            for i in c..n - m + 1 {
                perc_mult[i] += (n as f64 - m as f64 - i as f64 + 1.0).log2();
                y += ((self.coverage[i] as f64).log2() + perc_mult[i] - n_fall_m).exp2();
            }

            pangrowth[m - 1] = tot - y;
        }

        pangrowth
    }

    pub fn calc_growth_core(&self, t_coverage: &Threshold) -> Vec<f64> {
        let n = self.coverage.len() - 1; // hist array has length n+1: from 0..n (both included)
        let c = usize::max(1, t_coverage.to_absolute(n + 1));
        let mut n_fall_m: f64 = 0.0;
        let mut pangrowth: Vec<f64> = vec![0.0; n];

        // In perc_mult[i] is contained the percentage of combinations
        // that have an item of multiplicity i
        let mut perc_mult: Vec<f64> = Vec::with_capacity(n + 1);
        perc_mult.resize(n + 1, 0.0);

        for m in 1..n + 1 {
            let mut y: f64 = 0.0;
            n_fall_m += (n as f64 - m as f64 + 1.0).log2();
            for i in usize::max(m, c)..n + 1 {
                perc_mult[i] += (i as f64 - m as f64 + 1.0).log2();
                y += ((self.coverage[i] as f64).log2() + perc_mult[i] - n_fall_m).exp2();
            }
            pangrowth[m - 1] = y;
        }

        pangrowth
    }

    pub fn calc_growth_quorum(&self, t_coverage: &Threshold, t_quorum: &Threshold) -> Vec<f64> {
        let n = self.coverage.len() - 1; // hist array has length n+1: from [0..n]
        let c = usize::max(1, t_coverage.to_absolute(n));
        let quorum = t_quorum.to_relative(n);
        let mut pangrowth: Vec<f64> = vec![0.0; n];

        let mut n_fall_m: f64 = 0.0;
        let mut m_fact: f64 = 0.0;

        let mut perc_mult: Vec<f64> = vec![0.0; n + 1];
        let mut q: Vec<Vec<f64>> = vec![vec![0.0; n + 1]; n + 1];

        for m in 1..n + 1 {
            m_fact += (m as f64).log2();
            let m_quorum = (m as f64 * quorum).ceil() as usize;

            //100% quorum
            let mut yl: f64 = 0.0;
            n_fall_m += (n as f64 - m as f64 + 1.0).log2();
            for i in usize::max(m, c)..n + 1 {
                perc_mult[i] += (i as f64 - m as f64 + 1.0).log2();
                yl += ((self.coverage[i] as f64).log2() + perc_mult[i] - n_fall_m).exp2();
            }

            //[m_quorum, 100) quorum
            let mut yr: f64 = 0.0;
            for i in m_quorum..n {
                let mut sum_q = 0.0;
                let mut add = false;
                for j in usize::max(m_quorum, c)..m {
                    if n + j + 1 > i + m && j <= i {
                        if q[i][j] == 0.0 {
                            q[i][j] = choose(i, j);
                        }
                        q[i][j] += (n as f64 - i as f64 - m as f64 + 1.0 + j as f64).log2();
                        q[i][j] -= (m as f64 - j as f64).log2();
                        sum_q += (q[i][j] + m_fact - n_fall_m).exp2();
                        add = true;
                    }
                }
                if add {
                    yr += ((self.coverage[i] as f64).log2() + sum_q.log2()).exp2();
                }
            }
            pangrowth[m - 1] = yl + yr;
        }
        pangrowth
    }

    #[allow(dead_code)]
    pub fn to_tsv<W: std::io::Write>(&self, out: &mut std::io::BufWriter<W>) -> Result<(), Error> {
        writeln!(out, "hist\t{}", self.count)?;
        for (i, c) in self.coverage.iter().enumerate() {
            writeln!(out, "{}\t{}", i, c)?;
        }

        Ok(())
    }
}

pub enum RequireThreshold {
    Absolute,
    Relative,
    #[allow(dead_code)]
    Either,
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

pub struct ThresholdContainer {
    pub quorum: Vec<Threshold>,
    pub coverage: Vec<Threshold>,
}

impl ThresholdContainer {
    pub fn parse_params(quorum: &str, coverage: &str) -> Result<Self, Error> {
        let mut quorum_thresholds = Vec::new();
        if !quorum.is_empty() {
            quorum_thresholds = parse_threshold_cli(quorum, RequireThreshold::Relative)?;
            log::debug!(
                "loaded {} quorum thresholds: {}",
                quorum_thresholds.len(),
                quorum_thresholds
                    .iter()
                    .map(|t| format!("{}", t))
                    .collect::<Vec<String>>()
                    .join(", ")
            );
        }
        if quorum_thresholds.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "quorum threshold setting requires at least one element, but none is given",
            ));
        }

        let mut coverage_thresholds = Vec::new();
        if !coverage.is_empty() {
            coverage_thresholds = parse_threshold_cli(coverage, RequireThreshold::Absolute)?;
            log::debug!(
                "loaded {} coverage thresholds: {}",
                coverage_thresholds.len(),
                coverage_thresholds
                    .iter()
                    .map(|t| format!("{}", t))
                    .collect::<Vec<String>>()
                    .join(", ")
            );
        }
        if coverage_thresholds.is_empty() {
            return Err(Error::new(
                ErrorKind::InvalidData,
                "coverage threshold setting requires at least one element, but none is given",
            ));
        }

        if quorum_thresholds.len() != coverage_thresholds.len() {
            if quorum_thresholds.len() == 1 {
                quorum_thresholds = vec![quorum_thresholds[0]; coverage_thresholds.len()];
            } else if coverage_thresholds.len() == 1 {
                coverage_thresholds = vec![coverage_thresholds[0]; quorum_thresholds.len()];
            } else {
                return Err(Error::new(ErrorKind::InvalidData,
                        "number of coverage and quorum threshold must match, or either one must have a single value"));
            }
        }

        Ok(Self {
            quorum: quorum_thresholds,
            coverage: coverage_thresholds,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_almost_eq(a: f64, b: f64) {
        let epsilon = 1e-10;
        if (a - b).abs() > epsilon {
            panic!("Values are not almost equal: {} vs {}", a, b);
        }
    }

    fn factorial(n: usize) -> f64 {
        (1..=n).fold(1.0, |acc, x| acc * x as f64)
    }

    #[test]
    fn test_choose_function() {
        assert_almost_eq(choose(5, 0), 0.0);
        assert_almost_eq(choose(5, 5), 0.0);
        assert_almost_eq(choose(5, 1), (5.0f64).log2());
        assert_almost_eq(choose(5, 4), (5.0f64).log2());
        let expected = (factorial(5) / (factorial(2) * factorial(3))).log2();
        assert_almost_eq(choose(5, 2), expected);
        assert_eq!(choose(5, 6), 0.0);
    }

    #[test]
    fn test_hist_calc_growth_union() {
        let hist = Hist {
            count: CountType::Node,
            coverage: vec![0, 5, 3, 2],
        };

        let t_coverage = Threshold::Absolute(0);
        let test_growth: Vec<f64> = vec![5.666666666666667, 8.333333333333334, 10.0];
        let growth = hist.calc_growth_union(&t_coverage);
        assert_eq!(growth, test_growth, "Wrong growth union");
    }

    #[test]
    fn test_hist_calc_growth_core() {
        let hist = Hist {
            count: CountType::Node,
            coverage: vec![0, 5, 3, 2],
        };

        let t_coverage = Threshold::Absolute(0);
        let test_core: Vec<f64> = vec![5.666666666666666, 3.0, 2.0];
        let core = hist.calc_growth_core(&t_coverage);
        assert_eq!(core, test_core, "Wrong growth core");
    }

    #[test]
    fn test_hist_calc_growth_quorum() {
        let hist = Hist {
            count: CountType::Node,
            coverage: vec![0, 5, 3, 2, 3, 5, 0, 4, 2, 1],
        };

        let t_coverage = Threshold::Absolute(0);
        let t_quorum = Threshold::Relative(0.9);
        let test_growth: Vec<f64> = vec![
            11.88888888888889,
            7.027777777777777,
            4.761904761904761,
            3.4444444444444438,
            2.5952380952380953,
            2.0,
            1.5555555555555545,
            1.2222222222222217,
            1.0,
        ];
        let growth = hist.calc_growth_quorum(&t_coverage, &t_quorum);
        assert_eq!(growth, test_growth, "Wrong growth quorum");
    }
}
