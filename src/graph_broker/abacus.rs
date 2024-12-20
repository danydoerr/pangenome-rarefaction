/* standard use */
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::io::{Error, ErrorKind};
use std::iter::FromIterator;
use std::mem::take;
//use std::sync::{Arc, Mutex};

/* external crate*/
use itertools::Itertools;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet, VecDeque};

use crate::graph_broker::graph::{Edge, ItemId, Orientation};
/* private use */
use crate::io::*;
use crate::util::*;

use super::graph::{GraphStorage, PathSegment};
use super::util::{parse_gfa_paths_walks, parse_gfa_paths_walks_multiple};

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct GraphMaskParameters {
    pub positive_list: String,
    pub negative_list: String,
    pub groupby: String,
    pub groupby_sample: bool,
    pub groupby_haplotype: bool,
    pub order: Option<String>,
}

impl GraphMaskParameters {
    pub fn default() -> Self {
        Self {
            positive_list: "".to_owned(),
            negative_list: "".to_owned(),
            groupby: "".to_owned(),
            groupby_sample: false,
            groupby_haplotype: false,
            order: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GraphMask {
    pub groups: HashMap<PathSegment, String>,
    pub include_coords: Option<Vec<PathSegment>>,
    pub exclude_coords: Option<Vec<PathSegment>>,
    pub order: Option<Vec<PathSegment>>,
}

impl GraphMask {
    pub fn from_datamgr(
        params: &GraphMaskParameters,
        graph_storage: &GraphStorage,
    ) -> Result<Self, Error> {
        let groups = GraphMask::load_groups(
            &params.groupby,
            params.groupby_haplotype,
            params.groupby_sample,
            graph_storage,
        )?;
        let include_coords = GraphMask::complement_with_group_assignments(
            GraphMask::load_coord_list(&params.positive_list)?,
            &groups,
        )?;
        let exclude_coords = GraphMask::complement_with_group_assignments(
            GraphMask::load_coord_list(&params.negative_list)?,
            &groups,
        )?;

        let order = if let Some(order) = &params.order {
            let maybe_order = GraphMask::complement_with_group_assignments(
                GraphMask::load_coord_list(order)?,
                &groups,
            )?;
            if let Some(o) = &maybe_order {
                // if order is given, check that it comprises all included coords
                let all_included_paths: Vec<PathSegment> = match &include_coords {
                    None => {
                        let exclude: HashSet<&PathSegment> = match &exclude_coords {
                            Some(e) => e.iter().collect(),
                            None => HashSet::new(),
                        };
                        graph_storage
                            .path_segments
                            .iter()
                            .filter_map(|x| {
                                if !exclude.contains(x) {
                                    Some(x.clear_coords())
                                } else {
                                    None
                                }
                            })
                            .collect()
                    }
                    Some(include) => include.iter().map(|x| x.clear_coords()).collect(),
                };
                let order_set: HashSet<&PathSegment> = HashSet::from_iter(o.iter());

                for p in all_included_paths.iter() {
                    if !order_set.contains(p) {
                        let msg =
                            format!("order list does not contain information about path {}", p);
                        log::error!("{}", &msg);
                        // let's not be that harsh, shall we?
                        // return Err(Error::new( ErrorKind::InvalidData, msg));
                    }
                }

                // check that groups are not scrambled in include
                let mut visited: HashSet<&str> = HashSet::new();
                let mut cur: &str = groups.get(&o[0]).unwrap();
                for p in o.iter() {
                    let g: &str = groups.get(p).unwrap();
                    if cur != g && !visited.insert(g) {
                        let msg = format!("order of paths contains fragmented groups: path {} belongs to group that is interspersed by one or more other groups", p);
                        log::error!("{}", &msg);
                        return Err(Error::new(ErrorKind::InvalidData, msg));
                    }
                    cur = g;
                }
            }
            maybe_order
        } else {
            None
        };

        //let n_groups = HashSet::<&String>::from_iter(groups.values()).len();
        //if n_groups > 65534 {
        //    return Err(Error::new(
        //        ErrorKind::Unsupported,
        //        format!(
        //            "data has {} path groups, but command is not supported for more than 65534",
        //            n_groups
        //        ),
        //    ));
        //}

        Ok(GraphMask {
            groups,
            include_coords,
            exclude_coords,
            order,
        })
    }

    pub fn complement_with_group_assignments(
        coords: Option<Vec<PathSegment>>,
        groups: &HashMap<PathSegment, String>,
    ) -> Result<Option<Vec<PathSegment>>, Error> {
        //
        // We allow coords to be defined via groups; the following code
        // 1. complements coords with path segments from group assignments
        // 2. checks that group-based coordinates don't have start/stop information
        //
        let mut group2paths: HashMap<String, Vec<PathSegment>> = HashMap::default();
        for (p, g) in groups.iter() {
            group2paths.entry(g.clone()).or_default().push(p.clone())
        }
        let path_to_group: HashMap<PathSegment, String> = groups
            .iter()
            .map(|(ps, g)| (ps.clear_coords(), g.clone()))
            .collect();

        match coords {
            None => Ok(None),
            Some(v) => {
                v.into_iter()
                    .map(|p| {
                        // check if path segment defined in coords associated with a specific path,
                        // it is not considered a group
                        if path_to_group.contains_key(&p.clear_coords()) {
                            Ok(vec![p])
                        } else if group2paths.contains_key(&p.id()) {
                            if p.coords().is_some() {
                                let msg = format!("invalid coordinate \"{}\": group identifiers are not allowed to have start/stop information!", &p);
                                log::error!("{}", &msg);
                                Err(Error::new( ErrorKind::InvalidData, msg))
                            } else {
                                let paths = group2paths.get(&p.id()).unwrap().clone();
                                log::debug!("complementing coordinate list with {} paths associted with group {}", paths.len(), p.id());
                                Ok(paths)
                            }
                        } else {
                            let msg = format!("unknown path/group {}", &p);
                            log::error!("{}", &msg);
                            // let's not be so harsh as to throw an error, ok?
                            // Err(Error::new(ErrorKind::InvalidData, msg))
                            Ok(Vec::new())
                        }
                    })
                    .collect::<Result<Vec<Vec<PathSegment>>, Error>>().map(|x| Some(x[..]
                    .concat()))
            }
        }
    }

    pub fn load_coord_list(file_name: &str) -> Result<Option<Vec<PathSegment>>, Error> {
        Ok(if file_name.is_empty() {
            None
        } else {
            log::info!("loading coordinates from {}", file_name);
            let mut data = BufReader::new(fs::File::open(file_name)?);
            let use_block_info = true;
            let coords = parse_bed_to_path_segments(&mut data, use_block_info);
            log::debug!("loaded {} coordinates", coords.len());
            Some(coords)
        })
    }

    fn load_groups(
        file_name: &str,
        groupby_haplotype: bool,
        groupby_sample: bool,
        graph_storage: &GraphStorage,
    ) -> Result<HashMap<PathSegment, String>, Error> {
        if groupby_haplotype {
            Ok(graph_storage
                .path_segments
                .iter()
                .map(|x| {
                    (
                        x.clear_coords(),
                        format!(
                            "{}#{}",
                            &x.sample,
                            &x.haplotype.as_ref().unwrap_or(&String::new())
                        ),
                    )
                })
                .collect())
        } else if groupby_sample {
            Ok(graph_storage
                .path_segments
                .iter()
                .map(|x| (x.clear_coords(), x.sample.clone()))
                .collect())
        } else if !file_name.is_empty() {
            log::info!("loading groups from {}", file_name);
            let mut data = BufReader::new(fs::File::open(file_name)?);
            let group_assignments = parse_groups(&mut data)?;
            let mut path_to_group = HashMap::default();
            for (i, (path, group)) in group_assignments.into_iter().enumerate() {
                let path_nocoords = path.clear_coords();
                match path_to_group.get(&path_nocoords) {
                    Some(g) => {
                        if g != &group {
                            let msg = format!(
                                "error in line {}: path {} cannot be assigned to more than one group, but is assigned to at least two groups: {}, {}",
                                i, &path_nocoords, &g, &group
                            );
                            log::error!("{}", &msg);
                            return Err(Error::new(ErrorKind::InvalidData, msg));
                        }
                    }
                    None => {
                        path_to_group.insert(path_nocoords, group);
                    }
                }
            }
            log::debug!("loaded {} group assignments", path_to_group.len());

            // augment the group assignments with yet unassigned path segments
            graph_storage.path_segments.iter().for_each(|x| {
                let path = x.clear_coords();
                path_to_group.entry(path).or_insert_with(|| x.id());
            });
            Ok(path_to_group)
        } else {
            log::info!("no explicit grouping instruction given, group paths by their IDs (sample ID+haplotype ID+seq ID)");
            Ok(graph_storage
                .path_segments
                .iter()
                .map(|x| (x.clear_coords(), x.id()))
                .collect())
        }
    }

    fn get_path_order<'a>(&'a self, path_segments: &[PathSegment]) -> Vec<(ItemIdSize, &'a str)> {
        // orders elements of path_segments by the order in graph_mask.include; the returned vector
        // maps indices of path_segments to the group identifier

        let mut group_to_paths: HashMap<&'a str, Vec<(ItemIdSize, &'a str)>> = HashMap::default();

        for (i, p) in path_segments.iter().enumerate() {
            let group: &'a str = self.groups.get(&p.clear_coords()).unwrap();
            group_to_paths
                .entry(group)
                .or_default()
                .push((i as ItemIdSize, group));
        }

        let order: Vec<&PathSegment> = if let Some(order) = &self.order {
            order.iter().collect()
        } else if let Some(include) = &self.include_coords {
            include.iter().collect()
        } else {
            let exclude: HashSet<&PathSegment> = match &self.exclude_coords {
                Some(e) => e.iter().collect(),
                None => HashSet::new(),
            };
            path_segments
                .iter()
                .filter(|x| !exclude.contains(x))
                .collect::<Vec<&PathSegment>>()
        };
        order
            .into_iter()
            .map(|p| {
                group_to_paths
                    .remove(&self.groups.get(&p.clear_coords()).unwrap()[..])
                    .unwrap_or_default()
            })
            .collect::<Vec<Vec<(ItemIdSize, &'a str)>>>()
            .concat()
    }

    #[allow(dead_code)]
    pub fn count_groups(&self) -> usize {
        HashSet::<&String>::from_iter(self.groups.values()).len()
    }

    pub fn build_subpath_map(
        path_segments: &[PathSegment],
    ) -> HashMap<String, Vec<(usize, usize)>> {
        // intervals are 0-based, and [start, end), see https://en.wikipedia.org/wiki/BED_(file_format)
        let mut res: HashMap<String, HashSet<(usize, usize)>> = HashMap::default();

        path_segments.iter().for_each(|x| {
            res.entry(x.id()).or_default().insert(match x.coords() {
                None => (0, usize::MAX),
                Some((i, j)) => (i, j),
            });
        });

        HashMap::from_iter(res.into_iter().map(|(pid, coords)| {
            let mut v: Vec<(usize, usize)> = coords.into_iter().collect();
            v.sort();
            let mut i = 1;
            // remove overlaps
            while i < v.len() {
                if v[i - 1].1 >= v[i].0 {
                    let x = v.remove(i);
                    v[i - 1].1 = std::cmp::max(v[i - 1].1, x.1);
                } else {
                    i += 1
                }
            }
            (pid, v)
        }))
    }

    pub fn load_optional_subsetting(
        &self,
        graph_storage: &GraphStorage,
        count: &CountType,
    ) -> (
        Option<IntervalContainer>,
        Option<ActiveTable>,
        HashMap<String, Vec<(usize, usize)>>,
        HashMap<String, Vec<(usize, usize)>>,
    ) {
        // *only relevant for bps count in combination with subset option*
        // this table stores the number of bps of nodes that are *partially* uncovered by subset
        // coordinates
        let subset_covered_bps: Option<IntervalContainer> =
            if count == &CountType::Bp && self.include_coords.is_some() {
                Some(IntervalContainer::new())
            } else {
                None
            };

        // this table stores information about excluded nodes *if* the exclude setting is used
        let exclude_table = self.exclude_coords.as_ref().map(|_| {
            ActiveTable::new(
                graph_storage.number_of_items(count) + 1,
                count == &CountType::Bp,
            )
        });

        // build "include" lookup table
        let include_map = match &self.include_coords {
            None => HashMap::default(),
            Some(coords) => Self::build_subpath_map(coords),
        };

        // build "exclude" lookup table
        let exclude_map = match &self.exclude_coords {
            None => HashMap::default(),
            Some(coords) => Self::build_subpath_map(coords),
        };

        (subset_covered_bps, exclude_table, include_map, exclude_map)
    }

    pub fn load_optional_subsetting_multiple(
        &self,
        graph_storage: &GraphStorage,
        count_types: &Vec<CountType>,
    ) -> (
        Option<IntervalContainer>,
        Vec<Option<ActiveTable>>,
        HashMap<String, Vec<(usize, usize)>>,
        HashMap<String, Vec<(usize, usize)>>,
    ) {
        // *only relevant for bps count in combination with subset option*
        // this table stores the number of bps of nodes that are *partially* uncovered by subset
        // coordinates
        let subset_covered_bps: Option<IntervalContainer> =
            if count_types.contains(&CountType::Bp) && self.include_coords.is_some() {
                Some(IntervalContainer::new())
            } else {
                None
            };

        // this table stores information about excluded nodes *if* the exclude setting is used
        let exclude_tables: Vec<_> = count_types
            .iter()
            .map(|count| {
                self.exclude_coords.as_ref().map(|_| {
                    ActiveTable::new(
                        graph_storage.number_of_items(count) + 1,
                        count == &CountType::Bp,
                    )
                })
            })
            .collect();

        // build "include" lookup table
        let include_map = match &self.include_coords {
            None => HashMap::default(),
            Some(coords) => Self::build_subpath_map(coords),
        };

        // build "exclude" lookup table
        let exclude_map = match &self.exclude_coords {
            None => HashMap::default(),
            Some(coords) => Self::build_subpath_map(coords),
        };

        (subset_covered_bps, exclude_tables, include_map, exclude_map)
    }
}

#[derive(Debug, Clone)]
pub struct AbacusByTotal {
    pub count: CountType,
    pub countable: Vec<CountSize>,
    pub uncovered_bps: Option<HashMap<ItemIdSize, usize>>,
    pub groups: Vec<String>,
}

impl AbacusByTotal {
    pub fn from_gfa<R: std::io::Read>(
        data: &mut BufReader<R>,
        graph_mask: &GraphMask,
        graph_storage: &GraphStorage,
        count_type: CountType,
    ) -> (Self, HashMap<PathSegment, (u32, u32)>) {
        let (item_table, exclude_table, subset_covered_bps, paths_len) =
            parse_gfa_paths_walks(data, graph_mask, graph_storage, &count_type);
        (
            Self::item_table_to_abacus(
                graph_mask,
                graph_storage,
                count_type,
                item_table,
                exclude_table,
                subset_covered_bps,
            ),
            paths_len,
        )
    }

    pub fn from_gfa_multiple<R: std::io::Read>(
        data: &mut BufReader<R>,
        graph_mask: &GraphMask,
        graph_storage: &GraphStorage,
        count_types: &Vec<CountType>,
    ) -> (Vec<Self>, HashMap<PathSegment, (u32, u32)>) {
        let (item_tables, exclude_tables, mut subset_covered_bps, path_lens) =
            parse_gfa_paths_walks_multiple(data, graph_mask, graph_storage, count_types);
        let mut item_tables = VecDeque::from(item_tables);
        let mut exclude_tables = VecDeque::from(exclude_tables);
        let mut subset_covered_bps: VecDeque<_> = count_types
            .iter()
            .map(|count| match count {
                &CountType::Bp if subset_covered_bps.is_some() => take(&mut subset_covered_bps),
                _ => None,
            })
            .collect();
        let abaci = count_types
            .iter()
            .map(|count| {
                Self::item_table_to_abacus(
                    graph_mask,
                    graph_storage,
                    *count,
                    item_tables.pop_front().unwrap(),
                    exclude_tables.pop_front().unwrap(),
                    subset_covered_bps.pop_front().unwrap(),
                )
            })
            .collect();
        (abaci, path_lens)
    }

    pub fn item_table_to_abacus(
        graph_mask: &GraphMask,
        graph_storage: &GraphStorage,
        count: CountType,
        item_table: ItemTable,
        exclude_table: Option<ActiveTable>,
        subset_covered_bps: Option<IntervalContainer>,
    ) -> Self {
        log::info!("counting abacus entries..");
        // first element in countable is "zero" element. It is ignored in counting
        let mut countable: Vec<CountSize> = vec![0; graph_storage.number_of_items(&count) + 1];
        // countable with ID "0" is special and should not be considered in coverage histogram
        countable[0] = CountSize::MAX;
        let mut last: Vec<ItemIdSize> =
            vec![ItemIdSize::MAX; graph_storage.number_of_items(&count) + 1];

        let mut groups = Vec::new();
        for (path_id, group_id) in graph_mask.get_path_order(&graph_storage.path_segments) {
            if groups.is_empty() || groups.last().unwrap() != group_id {
                groups.push(group_id.to_string());
            }
            AbacusByTotal::coverage(
                &mut countable,
                &mut last,
                &item_table,
                &exclude_table,
                path_id,
                groups.len() as ItemIdSize - 1,
            );
        }

        log::info!(
            "abacus has {} path groups and {} countables",
            groups.len(),
            countable.len() - 1
        );

        Self {
            count,
            countable,
            uncovered_bps: Some(quantify_uncovered_bps(
                &exclude_table,
                &subset_covered_bps,
                graph_storage,
            )),
            groups,
        }
    }

    // pub fn from_cdbg_gfa<R: std::io::Read>(
    //     data: &mut BufReader<R>,
    //     graph_mask: &GraphMask,
    //     graph_storage: &GraphStorage,
    //     k: usize,
    //     unimer: &Vec<usize>,
    // ) -> Self {
    //     let item_table = parse_cdbg_gfa_paths_walks(data, graph_mask, graph_storage, k);
    //     Self::k_plus_one_mer_table_to_abacus(item_table, &graph_mask, &graph_storage, k, unimer)
    // }

    // pub fn k_plus_one_mer_table_to_abacus(
    //     item_table: ItemTable,
    //     graph_mask: &GraphMask,
    //     graph_storage: &GraphStorage,
    //     k: usize,
    //     unimer: &Vec<usize>,
    // ) -> Self {
    //     log::info!("counting abacus entries..");

    //     let mut infix_eq_tables: [HashMap<u64, InfixEqStorage>; SIZE_T] =
    //         [(); SIZE_T].map(|_| HashMap::default());

    //     let mut groups = Vec::new();
    //     for (path_id, group_id) in graph_mask.get_path_order(&graph_storage.path_segments) {
    //         if groups.is_empty() || groups.last().unwrap() != group_id {
    //             groups.push(group_id.to_string());
    //         }
    //         AbacusByTotal::create_infix_eq_table(
    //             &item_table,
    //             &graph_storage,
    //             &mut infix_eq_tables,
    //             path_id,
    //             groups.len() as u32 - 1,
    //             k,
    //         );
    //     }
    //     ////DEBUG
    //     //for i in 0..SIZE_T {
    //     //    for (key, v) in &infix_eq_tables[i] {
    //     //        println!("{}: {:?} {} {} {}", bits2kmer(*key, k-1), v.edges, v.last_edge, v.last_group, v.sigma);
    //     //    }
    //     //}

    //     let m = (groups.len() + 1) * groups.len() / 2;
    //     let mut countable: Vec<CountSize> = vec![0; m];

    //     for i in 0..SIZE_T {
    //         for (_k_minus_one_mer, infix_storage) in &infix_eq_tables[i] {
    //             for edge_count in infix_storage.edges.iter() {
    //                 if *edge_count != 0 {
    //                     //println!("{:?} {} {} {} ", infix_storage.edges, infix_storage.last_edge, infix_storage.last_group, infix_storage.sigma);
    //                     let idx = ((infix_storage.sigma) * (infix_storage.sigma - 1) / 2
    //                         + edge_count
    //                         - 1) as usize;
    //                     countable[idx] += 1;
    //                     //if infix_storage.sigma == 1 {
    //                     //    println!("{}",bits2kmer(*k_minus_one_mer, k-1));
    //                     //}
    //                 }
    //             }
    //         }
    //     }
    //     //DEBUG
    //     //println!("{:?}", countable);

    //     for i in 1..unimer.len() {
    //         countable[((i + 1) * i / 2) - 1] += unimer[i] as u32;
    //         //countable[((i+1)*i/2) - 1] = unimer[i] as u32;
    //     }

    //     Self {
    //         count: CountType::Node,
    //         countable: countable,
    //         uncovered_bps: None,
    //         groups: groups,
    //     }
    // }

    // fn create_infix_eq_table(
    //     item_table: &ItemTable,
    //     _graph_storage: &GraphStorage,
    //     infix_eq_tables: &mut [HashMap<u64, InfixEqStorage>; SIZE_T],
    //     path_id: ItemIdSize,
    //     group_id: u32,
    //     k: usize,
    // ) {
    //     let infix_eq_tables_ptr = Wrap(infix_eq_tables);

    //     (0..SIZE_T).into_par_iter().for_each(|i| {
    //         let start = item_table.id_prefsum[i][path_id as usize] as usize;
    //         let end = item_table.id_prefsum[i][path_id as usize + 1] as usize;
    //         for j in start..end {
    //             let k_plus_one_mer = item_table.items[i][j];
    //             let infix = get_infix(k_plus_one_mer, k);
    //             let first_nt = (k_plus_one_mer >> (2 * k)) as u64;
    //             let last_nt = k_plus_one_mer & 0b11;
    //             //println!("{}", bits2kmer(infix, k)); // Be sure that the first is an A
    //             //println!("{}", bits2kmer(infix, k-1));
    //             let combined_nt = ((first_nt << 2) | last_nt) as u8;
    //             unsafe {
    //                 (*infix_eq_tables_ptr.0)[i]
    //                     .entry(infix)
    //                     .and_modify(|infix_storage| {
    //                         if infix_storage.last_group == group_id
    //                             && infix_storage.last_edge != combined_nt
    //                             && infix_storage.last_edge != 255
    //                         {
    //                             //if infix_storage.last_group == group_id && infix_storage.last_edge != 255 {
    //                             infix_storage.edges[infix_storage.last_edge as usize] -= 1;
    //                             infix_storage.last_edge = 255;
    //                         } else if infix_storage.last_group != group_id {
    //                             infix_storage.last_edge = combined_nt;
    //                             infix_storage.edges[infix_storage.last_edge as usize] += 1;
    //                             infix_storage.last_group = group_id;
    //                             infix_storage.sigma += 1;
    //                         }
    //                     })
    //                     .or_insert_with(|| {
    //                         let mut infix_storage = InfixEqStorage::new();
    //                         infix_storage.last_edge = combined_nt;
    //                         infix_storage.edges[infix_storage.last_edge as usize] += 1;
    //                         infix_storage.last_group = group_id;
    //                         infix_storage.sigma = 1;
    //                         infix_storage
    //                     });
    //             }
    //         }
    //     });
    // }

    pub fn coverage(
        countable: &mut Vec<CountSize>,
        last: &mut Vec<ItemIdSize>,
        item_table: &ItemTable,
        exclude_table: &Option<ActiveTable>,
        path_id: ItemIdSize,
        group_id: ItemIdSize,
    ) {
        let countable_ptr = Wrap(countable);
        let last_ptr = Wrap(last);

        // Parallel node counting
        (0..SIZE_T).into_par_iter().for_each(|i| {
            let start = item_table.id_prefsum[i][path_id as usize] as usize;
            let end = item_table.id_prefsum[i][path_id as usize + 1] as usize;
            for j in start..end {
                let sid = item_table.items[i][j] as usize;
                unsafe {
                    if last[sid] != group_id
                        && (exclude_table.is_none() || !exclude_table.as_ref().unwrap().items[sid])
                    {
                        (*countable_ptr.0)[sid] += 1;
                        (*last_ptr.0)[sid] = group_id;
                    }
                }
            }
        });
    }

    pub fn construct_hist(&self) -> Vec<usize> {
        log::info!("constructing histogram..");
        // hist must be of size = num_groups + 1; having an index that starts
        // from 1, instead of 0, makes easier the calculation in hist2pangrowth.
        let mut hist: Vec<usize> = vec![0; self.groups.len() + 1];

        for (i, cov) in self.countable.iter().enumerate() {
            if *cov as usize >= hist.len() {
                if i != 0 {
                    log::warn!("coverage {} of item {} exceeds the number of groups {}, it'll be ignored in the count", cov, i, self.groups.len());
                }
            } else {
                hist[*cov as usize] += 1;
            }
        }
        hist
    }

    pub fn construct_hist_bps(&self, graph_storage: &GraphStorage) -> Vec<usize> {
        log::info!("constructing bp histogram..");
        // hist must be of size = num_groups + 1; having an index that starts
        // from 1, instead of 0, makes easier the calculation in hist2pangrowth.
        let mut hist: Vec<usize> = vec![0; self.groups.len() + 1];
        for (id, cov) in self.countable.iter().enumerate() {
            if *cov as usize >= hist.len() {
                if id != 0 {
                    log::info!("coverage {} of item {} exceeds the number of groups {}, it'll be ignored in the count", cov, id, self.groups.len());
                }
            } else {
                hist[*cov as usize] += graph_storage.node_lens[id] as usize;
            }
        }

        // subtract uncovered bps
        let uncovered_bps = self.uncovered_bps.as_ref().unwrap();
        for (id, uncov) in uncovered_bps.iter() {
            hist[self.countable[*id as usize] as usize] -= uncov;
            // add uncovered bps to 0-coverage count
            hist[0] += uncov;
        }
        hist
    }
}

#[derive(Debug, Clone)]
pub struct AbacusByGroup {
    pub count: CountType,
    pub r: Vec<usize>,
    pub v: Option<Vec<CountSize>>,
    pub c: Vec<GroupSize>,
    pub uncovered_bps: HashMap<ItemIdSize, usize>,
    pub groups: Vec<String>,
    // pub graph_storage: &'a GraphStorage,
}

impl AbacusByGroup {
    pub fn from_gfa<R: std::io::Read>(
        data: &mut std::io::BufReader<R>,
        graph_mask: &GraphMask,
        graph_storage: &GraphStorage,
        count: CountType,
        report_values: bool,
    ) -> Result<Self, Error> {
        log::info!("parsing path + walk sequences");
        let (item_table, exclude_table, subset_covered_bps, _paths_len) =
            parse_gfa_paths_walks(data, graph_mask, graph_storage, &count);

        let mut path_order: Vec<(ItemIdSize, GroupSize)> = Vec::new();
        let mut groups: Vec<String> = Vec::new();

        for (path_id, group_id) in graph_mask.get_path_order(&graph_storage.path_segments) {
            log::debug!(
                "processing path {} (group {})",
                &graph_storage.path_segments[path_id as usize],
                group_id
            );
            if groups.is_empty() || groups.last().unwrap() != group_id {
                groups.push(group_id.to_string());
            }
            //if groups.len() > 65534 {
            //    panic!("data has more than 65534 path groups, but command is not supported for more than 65534");
            //}
            path_order.push((path_id, (groups.len() - 1) as GroupSize));
        }

        let r = AbacusByGroup::compute_row_storage_space(
            &item_table,
            &exclude_table,
            &path_order,
            graph_storage.number_of_items(&count),
        );
        let (v, c) =
            AbacusByGroup::compute_column_values(&item_table, &path_order, &r, report_values);
        log::info!(
            "abacus has {} path groups and {} countables",
            groups.len(),
            r.len()
        );

        Ok(Self {
            count,
            r,
            v,
            c,
            uncovered_bps: quantify_uncovered_bps(
                &exclude_table,
                &subset_covered_bps,
                graph_storage,
            ),
            groups,
        })
    }

    fn compute_row_storage_space(
        item_table: &ItemTable,
        exclude_table: &Option<ActiveTable>,
        path_order: &Vec<(ItemIdSize, GroupSize)>,
        n_items: usize,
    ) -> Vec<usize> {
        log::info!("computing space allocating storage for group-based coverage table:");
        let mut last: Vec<GroupSize> = vec![GroupSize::MAX; n_items + 1];
        let last_ptr = Wrap(&mut last);

        let mut r: Vec<usize> = vec![0; n_items + 2];
        let r_ptr = Wrap(&mut r);
        for (path_id, group_id) in path_order {
            (0..SIZE_T).into_par_iter().for_each(|i| {
                let start = item_table.id_prefsum[i][*path_id as usize] as usize;
                let end = item_table.id_prefsum[i][*path_id as usize + 1] as usize;
                for j in start..end {
                    let sid = item_table.items[i][j] as usize;
                    if &last[sid] != group_id
                        && (exclude_table.is_none() || !exclude_table.as_ref().unwrap().items[sid])
                    {
                        unsafe {
                            (*r_ptr.0)[sid] += 1;
                            (*last_ptr.0)[sid] = *group_id;
                        }
                    }
                }
            });
        }
        log::info!(" ++ assigning storage locations");
        let mut c = 0;
        // can this be simplified?
        for item in &mut r {
            let tmp = *item;
            *item = c;
            c += tmp;
        }
        log::info!(
            " ++ group-aware table has {} non-zero elements",
            r.last().unwrap()
        );
        r
    }

    fn compute_column_values(
        item_table: &ItemTable,
        path_order: &Vec<(ItemIdSize, GroupSize)>,
        r: &[usize],
        report_values: bool,
    ) -> (Option<Vec<CountSize>>, Vec<GroupSize>) {
        let n = { *r.last().unwrap() };
        log::info!("allocating storage for group-based coverage table..");
        let mut v = if report_values {
            vec![0; n]
        } else {
            // we produce a dummy
            vec![0; 1]
        };
        let mut c: Vec<GroupSize> = vec![GroupSize::MAX; n];
        log::info!("done");

        log::info!("computing group-based coverage..");
        let v_ptr = Wrap(&mut v);
        let c_ptr = Wrap(&mut c);

        // group id is monotone increasing from 0 to #groups
        for (path_id, group_id) in path_order {
            let path_id_u = *path_id as usize;
            (0..SIZE_T).into_par_iter().for_each(|i| {
                let start = item_table.id_prefsum[i][path_id_u] as usize;
                let end = item_table.id_prefsum[i][path_id_u + 1] as usize;
                for j in start..end {
                    let sid = item_table.items[i][j] as usize;
                    let cv_start = r[sid];
                    let mut cv_end = r[sid + 1];
                    if cv_end != cv_start {
                        // look up storage location for node cur_sid: we use the last position
                        // of interval cv_start..cv_end, which is associated to coverage counts
                        // of the current node (sid), in the "c" array as pointer to the
                        // current column (group) / value (coverage) position. If the current group
                        // id does not match the one associated with the current position, we move
                        // on to the next. If cv_start + p == cv_end - 1, this means that we are
                        // currently writing the last element in that interval, and we need to make
                        // sure that we are no longer using it as pointer.
                        if cv_end - 1 > c.len() {
                            log::error!(
                                "oops, cv_end-1 is larger than the length of c for sid={}",
                                sid
                            );
                            cv_end = c.len() - 1;
                        }

                        let mut p = c[cv_end - 1] as usize;
                        unsafe {
                            // we  look at an untouched interval, so let's get the pointer game
                            // started...
                            if c[cv_end - 1] == GroupSize::MAX {
                                (*c_ptr.0)[cv_start] = *group_id;
                                // if it's just a single value in this interval, the pointer game
                                // ends before it started
                                if cv_start < cv_end - 1 {
                                    (*c_ptr.0)[cv_end - 1] = 0;
                                }
                                if report_values {
                                    (*v_ptr.0)[cv_start] += 1;
                                }
                            } else if cv_start + p < cv_end - 1 {
                                // if group id of current slot does not match current group id
                                // (remember group id's are strictly monotically increasing), then
                                // move on to the next slot
                                if c[cv_start + p] < *group_id {
                                    // move on to the next slot
                                    (*c_ptr.0)[cv_end - 1] += 1;
                                    // update local pointer
                                    p += 1;
                                    (*c_ptr.0)[cv_start + p] = *group_id
                                }
                                if report_values {
                                    (*v_ptr.0)[cv_start + p] += 1;
                                }
                            } else if report_values {
                                // make sure it points to the last element and not beyond
                                (*v_ptr.0)[cv_end - 1] += 1;
                            }
                        }
                    }
                }
            });
        }
        log::info!("done");
        (if report_values { Some(v) } else { None }, c)
    }

    // why &self and not self? we could destroy abacus at this point.
    pub fn calc_growth(
        &self,
        t_coverage: &Threshold,
        t_quorum: &Threshold,
        node_lens: &Vec<u32>,
    ) -> Vec<f64> {
        let mut res = vec![0.0; self.groups.len()];

        let c = usize::max(1, t_coverage.to_absolute(self.groups.len()));
        let q = f64::max(0.0, t_quorum.to_relative(self.groups.len()));

        let mut it = self.r.iter().tuple_windows().enumerate();
        // ignore first entry
        it.next();
        for (i, (&start, &end)) in it {
            if end - start >= c {
                let mut k = start;
                for j in self.c[start] as usize..self.groups.len() {
                    if k < end - 1 && self.c[k + 1] as usize <= j {
                        k += 1
                    }
                    if k - start + 1 >= ((self.c[k] as f64 + 1.0) * q).ceil() as usize {
                        // we never need to look into the actual value in self.v, because we
                        // know it must be non-zero, which is sufficient
                        match self.count {
                            CountType::Node | CountType::Edge => res[j] += 1.0,
                            CountType::Bp => {
                                let uncovered =
                                    self.uncovered_bps.get(&(i as ItemIdSize)).unwrap_or(&0);
                                let covered = node_lens[i] as usize;
                                if uncovered > &covered {
                                    log::error!("oops, #uncovered bps ({}) is larger than #coverd bps ({}) for node with sid {})", &uncovered, &covered, i);
                                } else {
                                    res[j] += (covered - uncovered) as f64
                                }
                            }
                            CountType::All => unreachable!("inadmissible count type"),
                        }
                    }
                }
            }
        }
        res
    }

    #[allow(dead_code)]
    pub fn write_rcv<W: Write>(&self, out: &mut BufWriter<W>) -> Result<(), Error> {
        write!(out, "{}", self.r[0])?;
        for x in self.r[1..].iter() {
            write!(out, "\t{}", x)?;
        }
        writeln!(out)?;
        write!(out, "{}", self.c[0])?;
        for x in self.c[1..].iter() {
            write!(out, "\t{}", x)?;
        }
        writeln!(out)?;
        if let Some(v) = &self.v {
            write!(out, "{}", v[0])?;
            for x in v[1..].iter() {
                write!(out, "\t{}", x)?;
            }
            writeln!(out)?;
        };
        Ok(())
    }

    pub fn to_tsv<W: Write>(
        &self,
        total: bool,
        out: &mut BufWriter<W>,
        graph_storage: &GraphStorage,
    ) -> Result<(), Error> {
        // create mapping from numerical node ids to original node identifiers
        log::info!("reporting coverage table");
        let dummy = Vec::new();
        let id2node: Vec<&Vec<u8>> = vec![&dummy; graph_storage.node_count + 1];
        // TODO: fix issue
        //for (node, id) in graph_storage.node2id.iter() {
        //    id2node[id.0 as usize] = node;
        //}

        match self.count {
            CountType::Node | CountType::Bp => {
                write!(out, "node")?;
                if total {
                    write!(out, "\ttotal")?;
                } else {
                    for group in self.groups.iter() {
                        write!(out, "\t{}", group)?;
                    }
                }
                writeln!(out)?;

                let mut it = self.r.iter().tuple_windows().enumerate();
                // ignore first entry
                it.next();
                for (i, (&start, &end)) in it {
                    let bp = if self.count == CountType::Bp {
                        graph_storage.node_lens[i] as usize
                            - *self.uncovered_bps.get(&(i as ItemIdSize)).unwrap_or(&0)
                    } else {
                        1
                    };
                    write!(out, "{}", std::str::from_utf8(id2node[i]).unwrap())?;
                    if total {
                        // we never need to look into the actual value in self.v, because we
                        // know it must be non-zero, which is sufficient
                        writeln!(out, "\t{}", end - start)?;
                    } else {
                        let mut k = start;
                        for j in 0 as GroupSize..self.groups.len() as GroupSize {
                            if k == end || j < self.c[k] {
                                write!(out, "\t0")?;
                            } else if j == self.c[k] {
                                match &self.v {
                                    None => write!(out, "\t{}", bp),
                                    Some(v) => write!(out, "\t{}", v[k] as usize * bp),
                                }?;
                                k += 1;
                            }
                        }
                        writeln!(out)?;
                    }
                }
            }
            CountType::Edge => {
                if let Some(edge2id) = &graph_storage.edge2id {
                    let dummy_edge = Edge(
                        ItemId(0),
                        Orientation::default(),
                        ItemId(0),
                        Orientation::default(),
                    );
                    let mut id2edge: Vec<&Edge> = vec![&dummy_edge; graph_storage.edge_count + 1];
                    for (edge, id) in edge2id.iter() {
                        id2edge[id.0 as usize] = edge;
                    }

                    write!(out, "edge")?;
                    if total {
                        write!(out, "\ttotal")?;
                    } else {
                        for group in self.groups.iter() {
                            write!(out, "\t{}", group)?;
                        }
                    }
                    writeln!(out)?;

                    let mut it = self.r.iter().tuple_windows().enumerate();
                    // ignore first entry
                    it.next();
                    for (i, (&start, &end)) in it {
                        let edge = id2edge[i];
                        write!(
                            out,
                            "{}{}{}{}",
                            edge.1,
                            std::str::from_utf8(id2node[edge.0 .0 as usize]).unwrap(),
                            edge.3,
                            std::str::from_utf8(id2node[edge.2 .0 as usize]).unwrap(),
                        )?;
                        if total {
                            // we never need to look into the actual value in self.v, because we
                            // know it must be non-zero, which is sufficient
                            writeln!(out, "\t{}", end - start)?;
                        } else {
                            let mut k = start;
                            for j in 0 as GroupSize..self.groups.len() as GroupSize {
                                if k == end || j < self.c[k] {
                                    write!(out, "\t0")?;
                                } else if j == self.c[k] {
                                    match &self.v {
                                        None => write!(out, "\t1"),
                                        Some(v) => write!(out, "\t{}", v[j as usize]),
                                    }?;
                                    k += 1;
                                }
                            }
                            writeln!(out)?;
                        }
                    }
                }
            }
            CountType::All => unreachable!("inadmissible count type"),
        };

        Ok(())
    }
}

//pub enum Abacus<'a> {
//    Total(AbacusByTotal<'a>),
//    Group(AbacusByGroup<'a>),
//    Nil,
//}

pub fn quantify_uncovered_bps(
    exclude_table: &Option<ActiveTable>,
    subset_covered_bps: &Option<IntervalContainer>,
    graph_storage: &GraphStorage,
) -> HashMap<ItemIdSize, usize> {
    //
    // 1. if subset is specified, then the node-based coverage calculated by the coverage()
    //    function overestimates the total coverage, because even nodes that are only partially
    //    covered are counted, thus the coverage needs to be reduced by the amount of uncovered
    //    bps from partially covered nodes
    // 2. if exclude is specified, then the coverage is overestimated by the coverage()
    //    function because partially excluded nodes are not excluded in the coverage
    //    calculation, thus the bps coverage needs to be reduced by the amount of excluded bps
    //    from partially excluded nodes
    // 3. if subset AND exclude are specified, nodes that are COMPLETELY excluded have not been
    //    counted in coverage, so they should not be considered here; all other nodes that are
    //    partially excluded / subset have contributed to the overestimation of coverage, so
    //    the bps coverage needs to be reduced by the amount of excluded or not coverered by
    //    any subset interval
    let mut res = HashMap::default();

    if let Some(subset_map) = subset_covered_bps {
        for sid in subset_map.keys() {
            // ignore COMPETELY excluded nodes
            if exclude_table.is_none() || !exclude_table.as_ref().unwrap().items[sid.0 as usize] {
                let l = graph_storage.node_len(sid) as usize;
                let covered = subset_map.total_coverage(
                    sid,
                    &exclude_table
                        .as_ref()
                        .map(|ex| ex.get_active_intervals(sid, l)),
                );
                if covered > l {
                    log::error!("oops, total coverage {} is larger than node length {} for node {}, intervals: {:?}", covered, l, sid.0, subset_map.get(sid).unwrap());
                } else {
                    // report uncovered bps
                    res.insert(sid.0, l - covered);
                }
            }
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use std::io::Error;
    use tempfile::NamedTempFile;

    use super::*;

    #[test]
    fn test_view_params_default() {
        let expected = GraphMaskParameters {
            positive_list: String::new(),
            negative_list: String::new(),
            groupby: String::new(),
            groupby_haplotype: false,
            groupby_sample: false,
            order: None,
        };
        let calculated = GraphMaskParameters::default();
        assert_eq!(calculated, expected);
    }

    fn get_graph_storage_path_segments() -> GraphStorage {
        GraphStorage::from_path_segments(vec![
            PathSegment::from_str("s1#2#2"),
            PathSegment::from_str("s1#1#2"),
            PathSegment::from_str("s1#1#1"),
            PathSegment::from_str("s2#1#2"),
        ])
    }

    fn get_load_groups_expected_hashmap(groups: [&str; 4]) -> HashMap<PathSegment, String> {
        HashMap::from([
            (PathSegment::from_str("s1#1#1"), groups[0].to_string()),
            (PathSegment::from_str("s1#1#2"), groups[1].to_string()),
            (PathSegment::from_str("s1#2#2"), groups[2].to_string()),
            (PathSegment::from_str("s2#1#2"), groups[3].to_string()),
        ])
    }

    #[test]
    fn test_load_groups_haplotype() -> Result<(), Error> {
        let expected = get_load_groups_expected_hashmap(["s1#1", "s1#1", "s1#2", "s2#1"]);
        let graph_storage = get_graph_storage_path_segments();
        let calculated = GraphMask::load_groups("", true, false, &graph_storage)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    #[test]
    fn test_load_groups_sample() -> Result<(), Error> {
        let expected = get_load_groups_expected_hashmap(["s1", "s1", "s1", "s2"]);
        let graph_storage = get_graph_storage_path_segments();
        let calculated = GraphMask::load_groups("", false, true, &graph_storage)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    fn get_temporary_file_name_with_content(text: &str) -> Result<(NamedTempFile, String), Error> {
        let mut f = NamedTempFile::new()?;
        writeln!(f, "{}", text)?;
        let msg = "Could not get path as &str";
        let file_name = f
            .path()
            .to_str()
            .ok_or(Error::new(ErrorKind::Other, msg))?
            .to_string();
        Ok((f, file_name))
    }

    #[test]
    fn test_load_groups_file() -> Result<(), Error> {
        let expected = get_load_groups_expected_hashmap(["g1", "g2", "g1", "g2"]);
        let graph_storage = get_graph_storage_path_segments();
        let text = "s1#1#1\tg1
s1#1#2\tg2
s1#2#2\tg1
s2#1#2\tg2";
        let (_file, file_name) = get_temporary_file_name_with_content(text)?;
        let calculated = GraphMask::load_groups(&file_name, false, false, &graph_storage)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    #[test]
    fn test_load_groups_none() -> Result<(), Error> {
        let expected = get_load_groups_expected_hashmap(["s1#1#1", "s1#1#2", "s1#2#2", "s2#1#2"]);
        let graph_storage = get_graph_storage_path_segments();
        let calculated = GraphMask::load_groups("", false, false, &graph_storage)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    #[test]
    fn test_load_coord_list_none() -> Result<(), Error> {
        let expected = None;
        let calculated = GraphMask::load_coord_list("")?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    #[test]
    fn test_load_coord_list_file() -> Result<(), Error> {
        let expected = Some(vec![
            PathSegment::from_str("s1#1#1:0-99"),
            PathSegment::from_str("s1#1#1:25-109"),
        ]);
        let text = "s1#1#1\t0\t99
s1#1#1\t25\t109";
        let (_file, file_name) = get_temporary_file_name_with_content(text)?;
        let calculated = GraphMask::load_coord_list(&file_name)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    #[test]
    fn test_complement_with_group_assignments_no_coords() -> Result<(), Error> {
        let expected: Option<Vec<PathSegment>> = None;
        let groups = HashMap::new();
        let calculated = GraphMask::complement_with_group_assignments(None, &groups)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    fn get_path_segment_with_coordinates(start: usize, end: usize) -> PathSegment {
        PathSegment {
            sample: "1".to_string(),
            haplotype: Some("2".to_string()),
            seqid: Some("3".to_string()),
            start: Some(start),
            end: Some(end),
        }
    }

    #[test]
    fn test_complement_with_group_assignments_coord_with_path_name() -> Result<(), Error> {
        let expected = Some(vec![get_path_segment_with_coordinates(1, 3)]);
        let coords = Some(vec![get_path_segment_with_coordinates(1, 3)]);
        let groups = HashMap::from([(get_path_segment_with_coordinates(8, 6), "g1".to_string())]);
        let calculated = GraphMask::complement_with_group_assignments(coords, &groups)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    #[test]
    fn test_complement_with_group_assignments_coord_with_group_name() -> Result<(), Error> {
        let expected = Some(vec![get_path_segment_with_coordinates(8, 6)]);
        let coords = Some(vec![PathSegment::from_str("g1")]);
        let groups = HashMap::from([(get_path_segment_with_coordinates(8, 6), "g1".to_string())]);
        let calculated = GraphMask::complement_with_group_assignments(coords, &groups)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    #[test]
    fn test_complement_with_group_assignments_coord_with_group_name_invalid() {
        let groups = HashMap::from([
            (PathSegment::from_str("a#0"), "g1".to_string()),
            (PathSegment::from_str("b#0"), "g1".to_string()),
        ]);

        let coords = Some(vec![PathSegment::from_str("g1:1-5")]);
        let result = GraphMask::complement_with_group_assignments(coords, &groups);
        assert!(
            result.is_err(),
            "Expected error due to invalid group identifier with start/stop information"
        );
    }

    #[test]
    fn test_complement_with_group_assignments_coord_with_invalid_name() -> Result<(), Error> {
        let expected: Option<Vec<PathSegment>> = Some(Vec::new());
        let groups = HashMap::from([
            (PathSegment::from_str("a#0"), "g1".to_string()),
            (PathSegment::from_str("b#0"), "g1".to_string()),
        ]);

        let coords = Some(vec![PathSegment::from_str("invalid")]);
        let calculated = GraphMask::complement_with_group_assignments(coords, &groups)?;
        assert_eq!(calculated, expected);
        Ok(())
    }

    // fn setup_test_data_cdbg() -> (GraphStorage, Params, String) {
    //     let test_gfa_file = "test/cdbg.gfa";
    //     let graph_storage = GraphStorage::from_gfa(test_gfa_file, CountType::Node);
    //     let params = Params::test_default_histgrowth();
    //     (graph_storage, params, test_gfa_file.to_string())
    // }

    // #[test]
    // fn test_abacus_by_total_from_cdbg_gfa() {
    //     let (graph_storage, params, test_gfa_file) = setup_test_data_cdbg();
    //     let path_aux = GraphMask::from_params(&params, &graph_storage).unwrap();
    //     let test_abacus_by_total = AbacusByTotal {
    //         count: CountType::Node,
    //         countable: vec![CountSize::MAX, 6, 4, 4, 2, 1],
    //         uncovered_bps: Some(HashMap::default()),
    //         groups: vec![
    //             "a#1#h1".to_string(),
    //             "b#1#h1".to_string(),
    //             "c#1#h1".to_string(),
    //             "c#1#h2".to_string(),
    //             "c#2#h1".to_string(),
    //             "d#1#h1".to_string(),
    //         ],
    //     };

    //     let mut data = bufreader_from_compressed_gfa(test_gfa_file.as_str());
    //     let (abacus_by_total, _paths_len) =
    //         AbacusByTotal::from_gfa(&mut data, &path_aux, &graph_storage, CountType::Node);
    //     assert_eq!(
    //         abacus_by_total.count, test_abacus_by_total.count,
    //         "Expected CountType to match Node"
    //     );
    //     assert_eq!(
    //         abacus_by_total.countable, test_abacus_by_total.countable,
    //         "Expected same countable"
    //     );
    //     assert_eq!(
    //         abacus_by_total.uncovered_bps, test_abacus_by_total.uncovered_bps,
    //         "Expected empty uncovered bps"
    //     );
    //     assert_eq!(
    //         abacus_by_total.groups, test_abacus_by_total.groups,
    //         "Expected same groups"
    //     );
    // }

    // fn setup_test_data_chr_m(count_type: CountType) -> (GraphStorage, Params, String) {
    //     let test_gfa_file = "test/chrM_test.gfa";
    //     let graph_storage = GraphStorage::from_gfa(test_gfa_file, count_type);
    //     let params = Params::Histgrowth {
    //         gfa_file: test_gfa_file.to_string(),
    //         count: count_type,
    //         positive_list: String::new(),
    //         negative_list: String::new(),
    //         groupby: String::new(),
    //         groupby_haplotype: false,
    //         groupby_sample: true,
    //         coverage: "1".to_string(),
    //         quorum: "0".to_string(),
    //         hist: false,
    //         output_format: OutputFormat::Table,
    //         threads: 0,
    //     };

    //     (graph_storage, params, test_gfa_file.to_string())
    // }

    // #[test]
    // fn test_abacus_by_total_from_chr_m_node() {
    //     let count_type = CountType::Node;
    //     let (graph_storage, params, test_gfa_file) = setup_test_data_chr_m(count_type);
    //     let path_aux = GraphMask::from_params(&params, &graph_storage).unwrap();
    //     let test_abacus_by_total = AbacusByTotal {
    //         count: count_type,
    //         countable: vec![
    //             CountSize::MAX,
    //             3, 2, 1, 3, 1, 2, 3, 1, 2, 3, 2, 3, 2, 1, 3, 1, 3, 2, 3, 2, 3, 4,
    //             2, 2, 4, 3, 1, 4, 2, 2, 4, 3, 1, 4, 2, 2, 4, 1, 4, 1, 3, 4, 1, 3,
    //             4, 2, 2, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4,
    //             2, 2, 4, 1, 3, 4, 1, 3, 4, 2, 2, 4, 3, 1, 4, 1, 3, 4, 1, 3, 4, 1,
    //             3, 4, 1, 3, 4, 2, 2, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3,
    //             4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 2, 2, 4, 1, 3, 4, 2, 2, 4,
    //             2, 2, 4, 2, 2, 4, 3, 1, 4, 3, 1, 4, 3, 1, 4, 3, 1, 4, 3, 1, 4, 1,
    //         ],
    //         uncovered_bps: Some(HashMap::default()),
    //         groups: vec![
    //             "chm13".to_string(),
    //             "grch38".to_string(),
    //             "HG00438".to_string(),
    //             "HG00621".to_string(),
    //         ],
    //     };

    //     let mut data = bufreader_from_compressed_gfa(test_gfa_file.as_str());
    //     let (abacus_by_total, _paths_len) =
    //         AbacusByTotal::from_gfa(&mut data, &path_aux, &graph_storage, CountType::Node);
    //     assert_eq!(
    //         abacus_by_total.count, test_abacus_by_total.count,
    //         "Expected CountType to match Node"
    //     );
    //     assert_eq!(
    //         abacus_by_total.countable, test_abacus_by_total.countable,
    //         "Expected same countable"
    //     );
    //     assert_eq!(
    //         abacus_by_total.uncovered_bps, test_abacus_by_total.uncovered_bps,
    //         "Expected empty uncovered bps"
    //     );
    //     assert_eq!(
    //         abacus_by_total.groups, test_abacus_by_total.groups,
    //         "Expected same groups"
    //     );
    //     let test_hist = vec![0, 39, 29, 41, 45];
    //     let hist = abacus_by_total.construct_hist();
    //     assert_eq!(hist, test_hist, "Expected same hist");
    // }

    // #[test]
    // fn test_abacus_by_total_from_chr_m_edge() {
    //     let count_type = CountType::Edge;
    //     let (graph_storage, params, test_gfa_file) = setup_test_data_chr_m(count_type);
    //     let path_aux = GraphMask::from_params(&params, &graph_storage).unwrap();
    //     let test_abacus_by_total = AbacusByTotal {
    //         count: count_type,
    //         countable: vec![
    //             CountSize::MAX,
    //             2, 1, 2, 1, 2, 1, 1, 2, 1, 2, 1, 2, 2, 1, 2, 1, 2, 2, 1, 2,
    //             1, 1, 1, 2, 2, 2, 1, 2, 3, 2, 2, 2, 2, 3, 1, 3, 1, 2, 2, 2,
    //             2, 3, 1, 3, 1, 2, 2, 2, 2, 1, 3, 1, 1, 3, 1, 3, 1, 3, 1, 3,
    //             2, 2, 2, 2, 3, 1, 1, 3, 3, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 3,
    //             1, 3, 1, 3, 3, 1, 1, 3, 2, 2, 2, 2, 1, 3, 1, 3, 1, 3, 1, 3,
    //             2, 2, 2, 2, 1, 3, 3, 1, 3, 1, 1, 3, 1, 3, 1, 3, 1, 3, 1, 3,
    //             3, 1, 1, 3, 2, 2, 2, 2, 3, 1, 1, 3, 3, 1, 1, 3, 3, 1, 1, 3,
    //             3, 1, 1, 3, 1, 3, 1, 3, 3, 1, 1, 3, 3, 1, 1, 3, 3, 1, 1, 3,
    //             3, 1, 1, 3, 2, 2, 2, 2, 3, 1, 1, 3, 2, 2, 2, 2, 2, 2, 2, 2,
    //             2, 2, 2, 2, 1, 3, 3, 1, 3, 1, 3, 1, 1, 3, 3, 1, 1, 3, 3, 1,
    //             1, 3, 3, 1, 1,
    //         ],
    //         uncovered_bps: Some(HashMap::default()),
    //         groups: vec![
    //             "chm13".to_string(),
    //             "grch38".to_string(),
    //             "HG00438".to_string(),
    //             "HG00621".to_string(),
    //         ],
    //     };

    //     let mut data = bufreader_from_compressed_gfa(test_gfa_file.as_str());
    //     let (abacus_by_total, _paths_len) =
    //         AbacusByTotal::from_gfa(&mut data, &path_aux, &graph_storage, count_type);
    //     assert_eq!(
    //         abacus_by_total.count, test_abacus_by_total.count,
    //         "Expected CountType to match Edge"
    //     );
    //     assert_eq!(
    //         abacus_by_total.countable, test_abacus_by_total.countable,
    //         "Expected same countable"
    //     );
    //     assert_eq!(
    //         abacus_by_total.uncovered_bps, test_abacus_by_total.uncovered_bps,
    //         "Expected empty uncovered bps"
    //     );
    //     assert_eq!(
    //         abacus_by_total.groups, test_abacus_by_total.groups,
    //         "Expected same groups"
    //     );
    //     let test_hist = vec![0, 80, 59, 66, 0];
    //     let hist = abacus_by_total.construct_hist();
    //     assert_eq!(hist, test_hist, "Expected same hist");
    // }

    // #[test]
    // fn test_abacus_by_total_from_chr_m_bp() {
    //     let count_type = CountType::Bp;
    //     let (graph_storage, params, test_gfa_file) = setup_test_data_chr_m(count_type);
    //     let path_aux = GraphMask::from_params(&params, &graph_storage).unwrap();
    //     let test_abacus_by_total = AbacusByTotal {
    //         count: count_type,
    //         countable: vec![
    //             CountSize::MAX,
    //             3, 2, 1, 3, 1, 2, 3, 1, 2, 3, 2, 3, 2, 1, 3, 1, 3, 2, 3, 2,
    //             3, 4, 2, 2, 4, 3, 1, 4, 2, 2, 4, 3, 1, 4, 2, 2, 4, 1, 4, 1,
    //             3, 4, 1, 3, 4, 2, 2, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4,
    //             1, 3, 4, 1, 3, 4, 2, 2, 4, 1, 3, 4, 1, 3, 4, 2, 2, 4, 3, 1,
    //             4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 2, 2, 4, 1, 3, 4, 1,
    //             3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4, 1, 3, 4,
    //             1, 3, 4, 2, 2, 4, 1, 3, 4, 2, 2, 4, 2, 2, 4, 2, 2, 4, 3, 1,
    //             4, 3, 1, 4, 3, 1, 4, 3, 1, 4, 3, 1, 4, 1,
    //         ],
    //         uncovered_bps: Some(HashMap::default()),
    //         groups: vec![
    //             "chm13".to_string(),
    //             "grch38".to_string(),
    //             "HG00438".to_string(),
    //             "HG00621".to_string(),
    //         ],
    //     };

    //     let mut data = bufreader_from_compressed_gfa(test_gfa_file.as_str());
    //     let (abacus_by_total, _paths_len) =
    //         AbacusByTotal::from_gfa(&mut data, &path_aux, &graph_storage, count_type);
    //     assert_eq!(
    //         abacus_by_total.count, test_abacus_by_total.count,
    //         "Expected CountType to match Edge"
    //     );
    //     assert_eq!(
    //         abacus_by_total.countable, test_abacus_by_total.countable,
    //         "Expected same countable"
    //     );
    //     assert_eq!(
    //         abacus_by_total.uncovered_bps, test_abacus_by_total.uncovered_bps,
    //         "Expected empty uncovered bps"
    //     );
    //     assert_eq!(
    //         abacus_by_total.groups, test_abacus_by_total.groups,
    //         "Expected same groups"
    //     );
    //     let test_hist = vec![0, 616, 31, 601, 15949];
    //     let hist = abacus_by_total.construct_hist_bps(&graph_storage);
    //     assert_eq!(hist, test_hist, "Expected same hist");
    // }

    // fn setup_test_data() -> (GraphStorage, Params, String) {
    //     let test_gfa_file = "test/cdbg.gfa";
    //     let graph_storage = GraphStorage::from_gfa(test_gfa_file, CountType::Node);
    //     let params = Params::test_default_histgrowth();
    //     (graph_storage, params, test_gfa_file.to_string())
    // }

    // #[test]
    // fn test_path_auxilliary_from_params_success() {
    //     let (graph_storage, params, _) = setup_test_data();

    //     let path_aux = GraphMask::from_params(&params, &graph_storage);
    //     assert!(
    //         path_aux.is_ok(),
    //         "Expected successful creation of GraphMask"
    //     );

    //     let path_aux = path_aux.unwrap();
    //     dbg!(&path_aux.groups.len());
    //     assert_eq!(path_aux.groups.len(), 6); // number of paths == groups
    // }

    // #[test]
    // fn test_path_auxilliary_load_groups_by_sample() {
    //     let (graph_storage, _, _) = setup_test_data();

    //     let result = GraphMask::load_groups("", false, true, &graph_storage);
    //     assert!(
    //         result.is_ok(),
    //         "Expected successful group loading by sample"
    //     );
    //     let groups = result.unwrap();
    //     let mut group_count = HashSet::new();
    //     for (_, g) in groups {
    //         group_count.insert(g);
    //     }
    //     assert_eq!(group_count.len(), 4, "Expected one group per sample");
    // }

    // #[test]
    // fn test_path_auxilliary_load_groups_by_haplotype() {
    //     let (graph_storage, _, _) = setup_test_data();

    //     let result = GraphMask::load_groups("", true, false, &graph_storage);
    //     let groups = result.unwrap();
    //     let mut group_count = HashSet::new();
    //     for (_, g) in groups {
    //         group_count.insert(g);
    //     }
    //     assert_eq!(group_count.len(), 5, "Expected 5 groups based on haplotype");
    // }

    // #[test]
    // fn test_complement_with_group_assignments_valid() {
    //     let groups = HashMap::from([
    //         (PathSegment::from_str("a#1#h1"), "G1".to_string()),
    //         (PathSegment::from_str("b#1#h1"), "G1".to_string()),
    //         (PathSegment::from_str("c#1#h1"), "G2".to_string()),
    //     ]);

    //     let coords = Some(vec![PathSegment::from_str("G1")]);
    //     let result = GraphMask::complement_with_group_assignments(coords, &groups);
    //     assert!(
    //         result.is_ok(),
    //         "Expected successful complement with group assignments"
    //     );

    //     let complemented = result.unwrap();
    //     assert!(
    //         complemented.is_some(),
    //         "Expected Some(complemented) coordinates"
    //     );
    //     assert_eq!(
    //         complemented.unwrap().len(),
    //         2,
    //         "Expected 2 path segments in the complemented list"
    //     );
    // }

    // #[test]
    // fn test_build_subpath_map_with_overlaps() {
    //     let path_segments = vec![
    //         PathSegment::new(
    //             "sample".to_string(),
    //             "hap1".to_string(),
    //             "seq1".to_string(),
    //             Some(0),
    //             Some(100),
    //         ),
    //         PathSegment::new(
    //             "sample".to_string(),
    //             "hap1".to_string(),
    //             "seq1".to_string(),
    //             Some(50),
    //             Some(150),
    //         ),
    //         PathSegment::new(
    //             "sample".to_string(),
    //             "hap1".to_string(),
    //             "seq2".to_string(),
    //             Some(0),
    //             Some(100),
    //         ),
    //     ];

    //     let subpath_map = GraphMask::build_subpath_map(&path_segments);
    //     assert_eq!(
    //         subpath_map.len(),
    //         2,
    //         "Expected 2 sequences in the subpath map"
    //     );
    //     assert_eq!(
    //         subpath_map.get("sample#hap1#seq1").unwrap().len(),
    //         1,
    //         "Expected 1 non-overlapping interval for seq1"
    //     );
    //     assert_eq!(
    //         subpath_map.get("sample#hap1#seq2").unwrap().len(),
    //         1,
    //         "Expected 1 interval for seq2"
    //     );
    // }

    // #[test]
    // fn test_get_path_order_with_exclusions() {
    //     let (graph_storage, _, _) = setup_test_data();

    //     let path_aux = GraphMask {
    //         groups: GraphMask::load_groups("", false, false, &graph_storage).unwrap(),
    //         include_coords: None,
    //         exclude_coords: Some(vec![
    //             PathSegment::from_str("a#1#h1"),
    //             PathSegment::from_str("b#1#h1"),
    //             PathSegment::from_str("b#1#h1"),
    //         ]), //duplicates do not cause any error
    //         order: None,
    //     };
    //     let ordered_paths = path_aux.get_path_order(&graph_storage.path_segments);
    //     assert_eq!(
    //         ordered_paths.len(),
    //         4,
    //         "Expected 4 paths in the final order"
    //     );
    // }

    // #[test]
    // fn test_path_auxilliary_count_groups() {
    //     let path_aux = GraphMask {
    //         groups: HashMap::from([
    //             (PathSegment::from_str("a#1#h1"), "G1".to_string()),
    //             (PathSegment::from_str("b#1#h1"), "G1".to_string()),
    //             (PathSegment::from_str("c#1#h1"), "G2".to_string()),
    //         ]),
    //         include_coords: None,
    //         exclude_coords: None,
    //         order: None,
    //     };

    //     assert_eq!(path_aux.count_groups(), 2, "Expected 2 unique groups");
    // }
}
