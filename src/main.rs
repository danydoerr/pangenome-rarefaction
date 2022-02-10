/* standard use */
use std::fs;
use std::io;
use std::io::prelude::*;
use std::str::{self, FromStr};

/* crate use */
use clap::Parser;
use handlegraph::handle::Handle;
use quick_csv::Csv;
use rand::{seq::SliceRandom, thread_rng};
use rustc_hash::{FxHashMap, FxHashSet};

#[derive(clap::Parser, Debug)]
#[clap(
    version = "0.1",
    author = "Daniel Doerr <daniel.doerr@hhu.de>",
    about = "Calculate rarefaction statistics from pangenome graph"
)]
pub struct Command {
    #[clap(index = 1, help = "graph in GFA1 format", required = true)]
    pub graph: String,

    #[clap(
        index = 2,
        required = true,
        help = "file of samples; their order determines the cumulative count"
    )]
    pub samples: String,

    #[clap(
        short = 't',
        long = "type",
        help = "type: node or edge count",
        default_value = "nodes",
        possible_values = &["nodes", "edges"],
    )]
    pub count_type: String,

    #[clap(
        short = 'r',
        long = "permuted_repeats",
        help = "if larger 0, the haplotypes are not added in given order, but by a random permutation; the process is repeated a given number of times",
        default_value = "0"
    )]
    pub permute: usize,

    #[clap(
        short = 'f',
        long = "fix_first",
        help = "only relevant if permuted_repeats > 0; fixes the first haplotype to be the first haplotype in all permutations"
    )]
    pub fix_first: bool,
}

fn parse_gfa<R: io::Read>(
    mut data: io::BufReader<R>,
) -> FxHashMap<String, Vec<(String, Vec<Handle>)>> {
    let mut res: FxHashMap<String, Vec<(String, Vec<Handle>)>> = FxHashMap::default();
    let reader = Csv::from_reader(&mut data)
        .delimiter(b'\t')
        .flexible(true)
        .has_header(false);

    for row in reader.into_iter() {
        let row = row.unwrap();
        let mut row_it = row.bytes_columns();
        let fst_col = row_it.next().unwrap();
        if &[b'W'] == fst_col {
            let sample_id = str::from_utf8(row_it.next().unwrap()).unwrap();
            let hap_id = str::from_utf8(row_it.next().unwrap()).unwrap();
            let seq_id = str::from_utf8(row_it.next().unwrap()).unwrap();
            let seq_start = str::from_utf8(row_it.next().unwrap()).unwrap();
            let seq_end = str::from_utf8(row_it.next().unwrap()).unwrap();
            let walk_ident = format!(
                "{}#{}#{}:{}-{}",
                sample_id, hap_id, seq_id, seq_start, seq_end
            );
            log::info!("processing walk {}", walk_ident);

            let walk_data = row_it.next().unwrap();
            let walk = parse_walk(walk_data.to_vec())
                .unwrap_or_else(|e| panic!("Unable to parse walk for {}: {}", &walk_ident, e));
            res.entry(sample_id.to_lowercase().to_string())
                .or_insert(Vec::new())
                .push((hap_id.to_lowercase().to_string(), walk));
        } else if &[b'P'] == fst_col {
            let path_name = str::from_utf8(row_it.next().unwrap()).unwrap();
            let segments = path_name.split("#").collect::<Vec<&str>>();
            let sample_id =
                segments[0].to_string().split(".").collect::<Vec<&str>>()[0].to_string();
            let hap_id = if segments.len() > 1 {
                segments[1].to_string()
            } else {
                "".to_string()
            };
            let path_data = row_it.next().unwrap();

            log::info!("processing path {}", path_name);
            let path = parse_path(path_data.to_vec())
                .unwrap_or_else(|e| panic!("Unable to parse walk for {}: {}", &path_name, e));
            res.entry(sample_id.to_lowercase().to_string())
                .or_insert(Vec::new())
                .push((hap_id.to_lowercase().to_string(), path));
        }
    }

    res
}

fn parse_path(path_data: Vec<u8>) -> Result<Vec<Handle>, String> {
    let mut path: Vec<Handle> = Vec::new();

    let mut cur_el: Vec<u8> = Vec::new();
    for c in path_data {
        if c == b',' {
            let sid =
                usize::from_str(str::from_utf8(&cur_el[..cur_el.len() - 1]).unwrap()).unwrap();
            let is_rev = match cur_el.last().unwrap() {
                b'+' => Ok(false),
                b'-' => Ok(true),
                _ => Err(format!(
                    "unknown orientation '{}' of segment {}",
                    cur_el.last().unwrap(),
                    sid
                )),
            };
            if is_rev.is_ok() {
                path.push(Handle::pack(sid, is_rev.unwrap()));
            } else {
                return Err(is_rev.err().unwrap());
            }
            cur_el.clear();
        } else {
            cur_el.push(c);
        }
    }

    if !cur_el.is_empty() {
        let sid = usize::from_str(str::from_utf8(&cur_el[..cur_el.len() - 1]).unwrap()).unwrap();
        let is_rev = match cur_el.last().unwrap() {
            b'+' => Ok(false),
            b'-' => Ok(true),
            _ => Err(format!(
                "unknown orientation '{}' of segment {}",
                cur_el.last().unwrap(),
                sid
            )),
        };
        if is_rev.is_ok() {
            path.push(Handle::pack(sid, is_rev.unwrap()));
        } else {
            return Err(is_rev.err().unwrap());
        }
    }
    Ok(path)
}

fn parse_walk(walk_data: Vec<u8>) -> Result<Vec<Handle>, String> {
    let mut walk: Vec<Handle> = Vec::new();

    let mut cur_el: Vec<u8> = Vec::new();
    for c in walk_data {
        if (c == b'>' || c == b'<') && !cur_el.is_empty() {
            let sid = usize::from_str(str::from_utf8(&cur_el[1..]).unwrap()).unwrap();
            let is_rev = match cur_el[0] {
                b'>' => Ok(false),
                b'<' => Ok(true),
                _ => Err(format!(
                    "unknown orientation '{}' of segment {}",
                    cur_el[0], sid
                )),
            };
            if is_rev.is_ok() {
                walk.push(Handle::pack(sid, is_rev.unwrap()));
            } else {
                return Err(is_rev.err().unwrap());
            }
            cur_el.clear();
        }
        cur_el.push(c);
    }

    if !cur_el.is_empty() {
        let sid = usize::from_str(str::from_utf8(&cur_el[1..]).unwrap()).unwrap();
        let is_rev = match cur_el[0] {
            b'>' => Ok(false),
            b'<' => Ok(true),
            _ => Err(format!(
                "unknown orientation '{}' of segment {}",
                cur_el[0], sid
            )),
        };
        if is_rev.is_ok() {
            walk.push(Handle::pack(sid, is_rev.unwrap()));
        } else {
            return Err(is_rev.err().unwrap());
        }
    }
    Ok(walk)
}

fn read_samples<R: io::Read>(mut data: io::BufReader<R>) -> Vec<String> {
    let mut res = Vec::new();

    let reader = Csv::from_reader(&mut data)
        .delimiter(b'\t')
        .flexible(true)
        .has_header(false);
    for row in reader.into_iter() {
        let row = row.unwrap();
        let mut row_it = row.bytes_columns();
        res.push(str::from_utf8(row_it.next().unwrap()).unwrap().to_string());
    }

    res
}

fn cumulative_count_nodes(
    samples: &Vec<String>,
    paths: &FxHashMap<String, Vec<(String, Vec<Handle>)>>,
) -> Vec<(String, String, usize, usize, usize)> {
    let mut res: Vec<(String, String, usize, usize, usize)> = Vec::new();
    let mut visited: FxHashMap<u64, FxHashSet<usize>> = FxHashMap::default();

    let mut new: usize = 0;
    for sample_id in samples.iter() {
        log::info!("cmulative node count of sample {}", sample_id);
        match paths.get(&sample_id.to_lowercase()) {
            None => {
                log::info!("sample {} not found in GFA!", sample_id);
            }
            Some(l) => {
                let mut cur_hap = None;
                for (hap_id, seq) in l.iter() {
                    if cur_hap != Some(hap_id) {
                        if cur_hap != None {
                            let major = visited
                                .values()
                                .map(|x| if x.len() >= (res.len() + 1) / 2 { 1 } else { 0 })
                                .sum();
                            let shared = visited
                                .values()
                                .map(|x| if x.len() == (res.len() + 1) { 1 } else { 0 })
                                .sum();
                            res.push((
                                sample_id.clone(),
                                cur_hap.unwrap().clone(),
                                new,
                                major,
                                shared,
                            ));
                        }
                        cur_hap = Some(hap_id);
                    }
                    for v in seq.iter() {
                        let vid = v.unpack_number();
                        if visited.contains_key(&vid) {
                            let x = visited.get_mut(&vid).unwrap();
                            x.insert(res.len());
                        } else {
                            new += 1;
                            let mut x = FxHashSet::default();
                            x.insert(res.len());
                            visited.insert(vid, x);
                        }
                    }
                }
                let major = visited
                    .values()
                    .map(|x| if x.len() >= (res.len() + 1) / 2 { 1 } else { 0 })
                    .sum();
                let shared = visited
                    .values()
                    .map(|x| if x.len() == (res.len() + 1) { 1 } else { 0 })
                    .sum();
                res.push((
                    sample_id.clone(),
                    cur_hap.unwrap().clone(),
                    new,
                    major,
                    shared,
                ));
            }
        };
    }

    res
}

fn cumulative_count_edges(
    samples: &Vec<String>,
    paths: &FxHashMap<String, Vec<(String, Vec<Handle>)>>,
) -> Vec<(String, String, usize, usize, usize)> {
    let mut res: Vec<(String, String, usize, usize, usize)> = Vec::new();
    let mut visited: FxHashMap<(Handle, Handle), FxHashSet<usize>> = FxHashMap::default();

    let mut new = 0;
    for sample_id in samples.iter() {
        log::info!("cmulative node count of sample {}", sample_id);
        match paths.get(&sample_id.to_lowercase()) {
            None => {
                log::info!("sample {} not found in GFA!", sample_id);
            }
            Some(l) => {
                let mut cur_hap = None;
                for (hap_id, seq) in l.iter() {
                    if cur_hap != Some(hap_id) {
                        if cur_hap != None {
                            let major = visited
                                .values()
                                .map(|x| if x.len() >= (res.len() + 1) / 2 { 1 } else { 0 })
                                .sum();
                            let shared = visited
                                .values()
                                .map(|x| if x.len() == (res.len() + 1) { 1 } else { 0 })
                                .sum();
                            res.push((
                                sample_id.clone(),
                                cur_hap.unwrap().clone(),
                                new,
                                major,
                                shared,
                            ));
                        }
                        cur_hap = Some(hap_id);
                    }
                    for j in 0..seq.len() - 1 {
                        let v = seq[j];
                        let u = seq[j + 1];
                        let e = if (seq[j].is_reverse() && seq[j + 1].is_reverse())
                            || (v.is_reverse() != u.is_reverse()
                                && v.unpack_number() > u.unpack_number())
                        {
                            (u.forward(), v.forward())
                        } else {
                            (v, u)
                        };

                        if visited.contains_key(&e) {
                            let x = visited.get_mut(&e).unwrap();
                            x.insert(res.len());
                        } else {
                            new += 1;
                            let mut x = FxHashSet::default();
                            x.insert(res.len());
                            visited.insert(e, x);
                        }
                    }
                }
                let major = visited
                    .values()
                    .map(|x| if x.len() >= (res.len() + 1) / 2 { 1 } else { 0 })
                    .sum();
                let shared = visited
                    .values()
                    .map(|x| if x.len() == (res.len() + 1) { 1 } else { 0 })
                    .sum();
                res.push((
                    sample_id.clone(),
                    cur_hap.unwrap().clone(),
                    new,
                    major,
                    shared,
                ));
            }
        };
    }

    res
}

fn main() -> Result<(), io::Error> {
    env_logger::init();

    // print output to stdout
    let mut out = io::BufWriter::new(std::io::stdout());

    // initialize command line parser & parse command line arguments
    let params = Command::parse();

    let data = io::BufReader::new(fs::File::open(&params.graph)?);
    log::info!("loading graph from {}", params.graph);

    let mut paths = parse_gfa(data);
    log::info!(
        "identified a total of {} paths in {} samples",
        paths.values().map(|x| x.len()).sum::<usize>(),
        paths.len()
    );

    // sort paths by haplotype ID
    for (_, seqs) in paths.iter_mut() {
        seqs.sort();
    }

    let data = io::BufReader::new(fs::File::open(&params.samples)?);
    log::info!("loading samples from {}", params.samples);
    let mut samples = read_samples(data);

    if params.permute > 0 {
        writeln!(
            out,
            "iteration\t{}\t{}\t{}",
            vec!["cumulative_count"; params.permute].join("\t"),
            vec!["major"; params.permute].join("\t"),
            vec!["shared"; params.permute].join("\t"),
        )?;
        writeln!(
            out,
            "\t{}\t{}\t{}",
            (0..params.permute).map(|x| x.to_string()).collect::<Vec<String>>().join("\t"),
            (0..params.permute).map(|x| x.to_string()).collect::<Vec<String>>().join("\t"),
            (0..params.permute).map(|x| x.to_string()).collect::<Vec<String>>().join("\t"),
        )?;

        let mut count: Vec<Vec<(String, String, usize, usize, usize)>> = Vec::new();

        let mut rng = thread_rng();

        for l in 0..params.permute {
            if params.fix_first {
                if l == 0 {
                    log::info!(
                        "do cumulative count on {} permutations with sample ({}) being fixed at 1st position", 
                        params.permute, samples[0]);
                }
                samples[1..].shuffle(&mut rng);
            } else {
                if l == 0 {
                    log::info!("do cumulative count on {} permutations", params.permute);
                }
                samples.shuffle(&mut rng);
            }
            log::info!("iteration {}", l + 1);
            count.push(match &params.count_type[..] {
                "nodes" => cumulative_count_nodes(&samples, &paths),
                "edges" => cumulative_count_edges(&samples, &paths),
                _ => panic!("Unknown count type {}", params.count_type),
            });
        }

        for i in 0..count[0].len() {
            let mut sample_id = format!("{}", i);
            if i == 0 && params.fix_first {
                sample_id = format!("{}#{}", count[0][0].0, count[0][0].1);
            }
            writeln!(
                out,
                "{}\t{}\t{}\t{}",
                sample_id,
                count
                    .iter()
                    .map(move |x| format!("{}", x[i].2))
                    .collect::<Vec<String>>()
                    .join("\t"),
                count
                    .iter()
                    .map(move |x| format!("{}", x[i].3))
                    .collect::<Vec<String>>()
                    .join("\t"),
                count
                    .iter()
                    .map(move |x| format!("{}", x[i].4))
                    .collect::<Vec<String>>()
                    .join("\t")
            )?;
        }
    } else {
        log::info!("do cumulative count of samples in the order given by the input file");
        writeln!(out, "sample\thaplotype\tcumulative_count\tmajor\tshared")?;
        let count = match &params.count_type[..] {
            "nodes" => cumulative_count_nodes(&samples, &paths),
            "edges" => cumulative_count_edges(&samples, &paths),
            _ => panic!("Unknown count type {}", params.count_type),
        };

        for (sample_id, hap_id, new, major, shared) in count.iter() {
            writeln!(
                out,
                "{}\t{}\t{}\t{}\t{}",
                sample_id, hap_id, new, major, shared
            )?;
        }
    }

    out.flush()?;
    log::info!("done");
    Ok(())
}
