use itertools::Itertools;
use std::str::{self, FromStr};
use std::time::Instant;
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Read},
    sync::{atomic::AtomicU32, Arc, Mutex},
};

use rayon::prelude::*;

use crate::{
    graph_broker::Edge,
    util::{
        intersects, is_contained, ActiveTable, CountType, IntervalContainer, ItemTable, Wrap,
        SIZE_T,
    },
};

use super::{abacus::GraphMask, graph::GraphStorage, ItemId, Orientation, PathSegment};

pub fn parse_gfa_paths_walks_multiple<R: Read>(
    data: &mut BufReader<R>,
    graph_mask: &GraphMask,
    graph_storage: &GraphStorage,
    count_types: &Vec<CountType>,
) -> (
    Vec<ItemTable>,
    Vec<Option<ActiveTable>>,
    Option<IntervalContainer>,
    HashMap<PathSegment, (u32, u32)>,
) {
    log::info!("parsing path + walk sequences");
    let mut item_tables =
        vec![ItemTable::new(graph_storage.path_segments.len()); count_types.len()];

    let (mut subset_covered_bps, mut exclude_tables, include_map, exclude_map) =
        graph_mask.load_optional_subsetting_multiple(graph_storage, count_types);

    let mut num_path = 0;
    let complete: Vec<(usize, usize)> = vec![(0, usize::MAX)];
    let mut paths_len: HashMap<PathSegment, (u32, u32)> = HashMap::new();

    let mut buf = vec![];
    let timer = Instant::now();
    while data.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
        if buf[0] == b'P' || buf[0] == b'W' {
            let (path_seg, buf_path_seg) = match buf[0] {
                b'P' => parse_path_identifier(&buf),
                b'W' => parse_walk_identifier(&buf),
                _ => unreachable!(),
            };

            log::debug!("processing path {}", &path_seg);

            let include_coords = if graph_mask.include_coords.is_none() {
                &complete[..]
            } else {
                match include_map.get(&path_seg.id()) {
                    None => &[],
                    Some(coords) => {
                        log::debug!(
                            "found include coords {:?} for path segment {}",
                            &coords[..],
                            &path_seg.id()
                        );
                        &coords[..]
                    }
                }
            };
            let exclude_coords = if graph_mask.exclude_coords.is_none() {
                &[]
            } else {
                match exclude_map.get(&path_seg.id()) {
                    None => &[],
                    Some(coords) => {
                        log::debug!(
                            "found exclude coords {:?} for path segment {}",
                            &coords[..],
                            &path_seg.id()
                        );
                        &coords[..]
                    }
                }
            };

            let (start, end) = path_seg.coords().unwrap_or((0, usize::MAX));

            // do not process the path sequence if path is neither part of subset nor exclude
            if graph_mask.include_coords.is_some()
                && !intersects(include_coords, &(start, end))
                && !intersects(exclude_coords, &(start, end))
            {
                log::debug!("path {} does not intersect with subset coordinates {:?} nor with exclude coordinates {:?} and therefore is skipped from processing",
                    &path_seg, &include_coords, &exclude_coords);

                // update prefix sum
                for item_table in &mut item_tables {
                    for i in 0..SIZE_T {
                        item_table.id_prefsum[i][num_path + 1] +=
                            item_table.id_prefsum[i][num_path];
                    }
                }

                num_path += 1;
                buf.clear();
                continue;
            }

            // TODO: separate this step and do it twice (?)
            (0..count_types.len()).for_each(|i| {
                let count = count_types[i];
                if count != CountType::Edge
                    && (graph_mask.include_coords.is_none()
                        || is_contained(include_coords, &(start, end)))
                        && (graph_mask.exclude_coords.is_none()
                            || is_contained(exclude_coords, &(start, end)))
                {
                    log::debug!("path {} is fully contained within subset coordinates {:?} and is eligible for full parallel processing", path_seg, include_coords);
                    let ex = if exclude_coords.is_empty() {
                        None
                    } else {
                        exclude_tables[i].as_mut()
                    };
                    let (num_added_nodes, bp_len) = match buf[0] {
                        b'P' => parse_path_seq_update_tables(
                            buf_path_seg,
                            graph_storage,
                            &mut item_tables[i],
                            ex,
                            num_path,
                        ),
                        b'W' => parse_walk_seq_update_tables(
                            buf_path_seg,
                            graph_storage,
                            &mut item_tables[i],
                            ex,
                            num_path,
                        ),
                        _ => unreachable!(),
                    };
                    paths_len.insert(path_seg.clone(), (num_added_nodes, bp_len));
                } else {
                    let sids = match buf[0] {
                        b'P' => parse_path_seq_to_item_vec(buf_path_seg, graph_storage),
                        b'W' => parse_walk_seq_to_item_vec(buf_path_seg, graph_storage),
                        _ => unreachable!(),
                    };

                    match count {
                        CountType::Node | CountType::Bp => {
                            let (node_len, bp_len) = update_tables(
                                &mut item_tables[i],
                                &mut subset_covered_bps.as_mut(),
                                &mut exclude_tables[i].as_mut(),
                                num_path,
                                graph_storage,
                                sids,
                                include_coords,
                                exclude_coords,
                                start,
                            );
                            paths_len.insert(path_seg.clone(), (node_len as u32, bp_len as u32));
                        }
                        CountType::Edge => update_tables_edgecount(
                            &mut item_tables[i],
                            &mut exclude_tables[i].as_mut(),
                            num_path,
                            graph_storage,
                            sids,
                            include_coords,
                            exclude_coords,
                            start,
                        ),
                        CountType::All => unreachable!("inadmissable count type"),
                    };
                }
            });
            num_path += 1;
        }
        buf.clear();
    }
    let duration = timer.elapsed();
    log::info!(
        "func done; count: {:?}; time elapsed: {:?}",
        count_types,
        duration
    );
    (item_tables, exclude_tables, subset_covered_bps, paths_len)
}

pub fn parse_gfa_paths_walks<R: Read>(
    data: &mut BufReader<R>,
    graph_mask: &GraphMask,
    graph_storage: &GraphStorage,
    count: &CountType,
) -> (
    ItemTable,
    Option<ActiveTable>,
    Option<IntervalContainer>,
    HashMap<PathSegment, (u32, u32)>,
) {
    log::info!("parsing path + walk sequences");
    // TODO: item_table will be returned
    let mut item_table = ItemTable::new(graph_storage.path_segments.len());

    // TODO: subset_covered_bps and exclude_table will be returned
    let (mut subset_covered_bps, mut exclude_table, include_map, exclude_map) =
        graph_mask.load_optional_subsetting(graph_storage, count);

    let mut num_path = 0;
    let complete: Vec<(usize, usize)> = vec![(0, usize::MAX)];
    let mut paths_len: HashMap<PathSegment, (u32, u32)> = HashMap::new();

    let mut buf = vec![];
    let timer = Instant::now();
    while data.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
        if buf[0] == b'P' || buf[0] == b'W' {
            let (path_seg, buf_path_seg) = match buf[0] {
                b'P' => parse_path_identifier(&buf),
                b'W' => parse_walk_identifier(&buf),
                _ => unreachable!(),
            };

            log::debug!("processing path {}", &path_seg);

            let include_coords = if graph_mask.include_coords.is_none() {
                &complete[..]
            } else {
                match include_map.get(&path_seg.id()) {
                    None => &[],
                    Some(coords) => {
                        log::debug!(
                            "found include coords {:?} for path segment {}",
                            &coords[..],
                            &path_seg.id()
                        );
                        &coords[..]
                    }
                }
            };
            let exclude_coords = if graph_mask.exclude_coords.is_none() {
                &[]
            } else {
                match exclude_map.get(&path_seg.id()) {
                    None => &[],
                    Some(coords) => {
                        log::debug!(
                            "found exclude coords {:?} for path segment {}",
                            &coords[..],
                            &path_seg.id()
                        );
                        &coords[..]
                    }
                }
            };

            let (start, end) = path_seg.coords().unwrap_or((0, usize::MAX));

            // do not process the path sequence if path is neither part of subset nor exclude
            if graph_mask.include_coords.is_some()
                && !intersects(include_coords, &(start, end))
                && !intersects(exclude_coords, &(start, end))
            {
                log::debug!("path {} does not intersect with subset coordinates {:?} nor with exclude coordinates {:?} and therefore is skipped from processing",
                    &path_seg, &include_coords, &exclude_coords);

                // update prefix sum
                // TODO: do this for all 3 tables
                for i in 0..SIZE_T {
                    item_table.id_prefsum[i][num_path + 1] += item_table.id_prefsum[i][num_path];
                }

                num_path += 1;
                buf.clear();
                continue;
            }

            // TODO: separate this step and do it twice (?)
            if count != &CountType::Edge
                && (graph_mask.include_coords.is_none()
                    || is_contained(include_coords, &(start, end)))
                && (graph_mask.exclude_coords.is_none()
                    || is_contained(exclude_coords, &(start, end)))
            {
                log::debug!("path {} is fully contained within subset coordinates {:?} and is eligible for full parallel processing", path_seg, include_coords);
                let ex = if exclude_coords.is_empty() {
                    None
                } else {
                    exclude_table.as_mut()
                };
                let (num_added_nodes, bp_len) = match buf[0] {
                    b'P' => parse_path_seq_update_tables(
                        buf_path_seg,
                        graph_storage,
                        &mut item_table,
                        ex,
                        num_path,
                    ),
                    b'W' => parse_walk_seq_update_tables(
                        buf_path_seg,
                        graph_storage,
                        &mut item_table,
                        ex,
                        num_path,
                    ),
                    _ => unreachable!(),
                };
                paths_len.insert(path_seg, (num_added_nodes, bp_len));
            } else {
                let sids = match buf[0] {
                    b'P' => parse_path_seq_to_item_vec(buf_path_seg, graph_storage),
                    b'W' => parse_walk_seq_to_item_vec(buf_path_seg, graph_storage),
                    _ => unreachable!(),
                };

                match count {
                    CountType::Node | CountType::Bp => {
                        let (node_len, bp_len) = update_tables(
                            &mut item_table,
                            &mut subset_covered_bps.as_mut(),
                            &mut exclude_table.as_mut(),
                            num_path,
                            graph_storage,
                            sids,
                            include_coords,
                            exclude_coords,
                            start,
                        );
                        paths_len.insert(path_seg, (node_len as u32, bp_len as u32));
                    }
                    CountType::Edge => update_tables_edgecount(
                        &mut item_table,
                        &mut exclude_table.as_mut(),
                        num_path,
                        graph_storage,
                        sids,
                        include_coords,
                        exclude_coords,
                        start,
                    ),
                    CountType::All => unreachable!("inadmissable count type"),
                };
            }
            num_path += 1;
        }
        buf.clear();
    }
    let duration = timer.elapsed();
    log::info!(
        "func done; count: {:?}; time elapsed: {:?}",
        count,
        duration
    );
    (item_table, exclude_table, subset_covered_bps, paths_len)
}

pub fn parse_walk_identifier(data: &[u8]) -> (PathSegment, &[u8]) {
    let mut six_col: Vec<&str> = Vec::with_capacity(6);

    let mut it = data.iter();
    let mut i = 0;
    for _ in 0..6 {
        let j = it.position(|x| x == &b'\t').unwrap();
        six_col.push(str::from_utf8(&data[i..i + j]).unwrap());
        i += j + 1;
    }

    let seq_start = match six_col[4] {
        "*" => None,
        a => Some(usize::from_str(a).unwrap()),
    };

    let seq_end = match six_col[5] {
        "*" => None,
        a => Some(usize::from_str(a).unwrap()),
    };

    let path_seg = PathSegment::new(
        six_col[1].to_string(),
        six_col[2].to_string(),
        six_col[3].to_string(),
        seq_start,
        seq_end,
    );

    (path_seg, &data[i..])
}

pub fn parse_path_identifier(data: &[u8]) -> (PathSegment, &[u8]) {
    let mut iter = data.iter();

    let start = iter.position(|&x| x == b'\t').unwrap() + 1;
    let offset = iter.position(|&x| x == b'\t').unwrap();
    let path_name = str::from_utf8(&data[start..start + offset]).unwrap();
    (
        PathSegment::from_str(path_name),
        &data[start + offset + 1..],
    )
}

pub fn update_tables(
    item_table: &mut ItemTable,
    subset_covered_bps: &mut Option<&mut IntervalContainer>,
    exclude_table: &mut Option<&mut ActiveTable>,
    num_path: usize,
    graph_storage: &GraphStorage,
    path: Vec<(ItemId, Orientation)>,
    include_coords: &[(usize, usize)],
    exclude_coords: &[(usize, usize)],
    offset: usize,
) -> (usize, usize) {
    let mut i = 0;
    let mut j = 0;
    let mut p = offset;

    let mut included = 0;
    let mut included_bp = 0;
    let mut excluded = 0;

    log::debug!(
        "checking inclusion/exclusion criteria on {} nodes..",
        path.len()
    );
    if path.len() == 0 {
        return (included, included_bp);
    }

    for (sid, o) in &path {
        let l = graph_storage.node_len(&sid) as usize;

        // this implementation of include coords for bps is *not exact* as illustrated by the
        // following scenario:
        //
        //   subset intervals:           ____________________________
        //                ______________|_____________________________
        //               |
        //      ___________________________________________     ____
        //  ---|                some node                  |---|
        //      -------------------------------------------     ----
        //
        //
        //   what the following code does:
        //                ___________________________________________
        //               |
        //               |             coverage count
        //      ___________________________________________     ____
        //  ---|                some node                  |---|
        //      -------------------------------------------     ----
        //
        //
        // node count handling: node is only counted if *completely* covered by subset
        //
        //
        // update current pointer in include_coords list

        // end is not inclusive, so if end <= p (=offset) then advance to the next interval
        let mut stop_here = false;
        while i < include_coords.len() && include_coords[i].0 < p + l && !stop_here {
            if include_coords[i].1 > p {
                let mut a = if include_coords[i].0 > p {
                    include_coords[i].0 - p
                } else {
                    0
                };
                let mut b = if include_coords[i].1 < p + l {
                    // advance to the next interval
                    i += 1;
                    include_coords[i - 1].1 - p
                } else {
                    stop_here = true;
                    l
                };

                // reverse coverage interval in case of backward orientation
                if o == &Orientation::Backward {
                    (a, b) = (l - b, l - a);
                }

                let idx = (sid.0 as usize) % SIZE_T;
                item_table.items[idx].push(sid.0);
                item_table.id_prefsum[idx][num_path + 1] += 1;
                if let Some(int) = subset_covered_bps.as_mut() {
                    // if fully covered, we do not need to store anything in the map
                    if b - a == l {
                        if int.contains(sid) {
                            int.remove(sid);
                        }
                    } else {
                        int.add(*sid, a, b);
                    }
                }
                included += 1;
                included_bp += b - a;
            } else {
                // advance to the next interval
                i += 1;
            }
        }

        let mut stop_here = false;
        while j < exclude_coords.len() && exclude_coords[j].0 < p + l && !stop_here {
            if exclude_coords[j].1 > p {
                let mut a = if exclude_coords[j].0 > p {
                    exclude_coords[j].0 - p
                } else {
                    0
                };
                let mut b = if exclude_coords[j].1 < p + l {
                    // advance to the next interval for the next iteration
                    j += 1;
                    exclude_coords[j - 1].1 - p
                } else {
                    stop_here = true;
                    l
                };

                // reverse coverage interval in case of backward orientation
                if o == &Orientation::Backward {
                    (a, b) = (l - b, l - a);
                }

                if let Some(map) = exclude_table {
                    if map.with_annotation() {
                        map.activate_n_annotate(*sid, l, a, b)
                            .expect("this error should never occur");
                    } else {
                        map.activate(&sid);
                    }
                    excluded += 1;
                }
            } else {
                j += 1;
            }
        }

        if i >= include_coords.len() && j >= exclude_coords.len() {
            // terminate parse if all "include" and "exclude" coords are processed
            break;
        }
        p += l;
    }

    log::debug!(
        "found {} included nodes ({} included bps) and {} excluded nodes, and discarded the rest",
        included,
        included_bp,
        excluded,
    );

    // Compute prefix sum
    for i in 0..SIZE_T {
        item_table.id_prefsum[i][num_path + 1] += item_table.id_prefsum[i][num_path];
    }
    log::debug!("..done");
    (included, included_bp)
}

pub fn update_tables_edgecount(
    item_table: &mut ItemTable,
    exclude_table: &mut Option<&mut ActiveTable>,
    num_path: usize,
    graph_storage: &GraphStorage,
    path: Vec<(ItemId, Orientation)>,
    include_coords: &[(usize, usize)],
    exclude_coords: &[(usize, usize)],
    offset: usize,
) {
    let mut i = 0;
    let mut j = 0;
    let mut p = offset;

    // edges are positioned between nodes, offset by the first node
    if !path.is_empty() {
        p += graph_storage.node_len(&path[0].0) as usize;
    }

    log::debug!("checking inclusion/exclusion criteria on {} nodes, inserting successful candidates to corresponding data structures..", path.len());

    for ((sid1, o1), (sid2, o2)) in path.into_iter().tuple_windows() {
        // update current pointer in include_coords list
        while i < include_coords.len() && include_coords[i].1 <= p {
            i += 1;
        }

        // update current pointer in exclude_coords list
        while j < exclude_coords.len() && exclude_coords[j].1 <= p {
            j += 1;
        }

        let l = graph_storage.node_len(&sid2) as usize;

        let e = Edge::canonical(sid1, o1, sid2, o2);
        let eid = graph_storage
            .edge2id
            .as_ref()
            .expect("update_tables_edgecount requires edge2id map in GraphStorage")
            .get(&e)
            .unwrap_or_else(|| {
                panic!(
                    "unknown edge {}. Is flipped edge known? {}",
                    &e,
                    if graph_storage
                        .edge2id
                        .as_ref()
                        .unwrap()
                        .contains_key(&e.flip())
                    {
                        "Yes"
                    } else {
                        "No"
                    }
                )
            });
        // check if the current position fits within active segment
        if i < include_coords.len() && include_coords[i].0 < p + l {
            let idx = (eid.0 as usize) % SIZE_T;
            item_table.items[idx].push(eid.0);
            item_table.id_prefsum[idx][num_path + 1] += 1;
        }
        if exclude_table.is_some() && j < exclude_coords.len() && exclude_coords[j].0 < p + l {
            exclude_table.as_mut().unwrap().activate(eid);
        } else if i >= include_coords.len() && j >= exclude_coords.len() {
            // terminate parse if all "include" and "exclude" coords are processed
            break;
        }
        p += l;
    }
    // Compute prefix sum
    for i in 0..SIZE_T {
        item_table.id_prefsum[i][num_path + 1] += item_table.id_prefsum[i][num_path];
    }
    log::debug!("..done");
}

pub fn parse_walk_seq_to_item_vec(
    data: &[u8],
    graph_storage: &GraphStorage,
) -> Vec<(ItemId, Orientation)> {
    // later codes assumes that data is non-empty...
    if data.is_empty() {
        return Vec::new();
    }

    // whatever the orientation of the first node is, will be used to split the sequence first;
    // this ensures that the first split results in an empty sequence at the beginning
    let s1 = Orientation::from_lg(data[0]);
    let s2 = s1.flip();

    let mut it = data.iter();
    let end = it
        .position(|x| x == &b'\t' || x == &b'\n' || x == &b'\r')
        .unwrap();

    log::debug!("parsing walk sequences of size {}..", end);

    // ignore first > | < so that no empty is created for 1st node
    let sids: Vec<(ItemId, Orientation)> = data[..end]
        .par_split(|x| &s1 == x)
        .map(|x| {
            if x.is_empty() {
                // not nice... but Rust expects struct `std::iter::Once<(ItemIdSize, util::Orientation)>`
                //
                // this case shouldn't occur too often, so should be fine in terms for runtime
                vec![]
            } else {
                let i = x.iter().position(|z| &s2 == z).unwrap_or(x.len());
                let sid = (
                    graph_storage.get_node_id(&x[..i]).unwrap_or_else(|| {
                        panic!(
                            "walk contains unknown node {{{}}}'",
                            str::from_utf8(&x[..i]).unwrap()
                        )
                    }),
                    s1,
                );
                if i < x.len() {
                    // not nice... but Rust expects struct `std::iter::Once<(ItemIdSize, util::Orientation)>`
                    //
                    // this case can happen more frequently... hopefully it doesn't blow up the
                    // runtime
                    [sid]
                        .into_par_iter()
                        .chain(
                            x[i + 1..]
                                .par_split(|y| &s2 == y)
                                .map(|y| {
                                    if y.is_empty() {
                                        vec![]
                                    } else {
                                        vec![(
                                            graph_storage.get_node_id(y).unwrap_or_else(|| {
                                                panic!(
                                                    "walk contains unknown node {{{}}}",
                                                    str::from_utf8(y).unwrap()
                                                )
                                            }),
                                            s2,
                                        )]
                                    }
                                })
                                .flatten(),
                        )
                        .collect()
                } else {
                    vec![sid]
                }
            }
        })
        .flatten()
        .collect();
    log::debug!("..done");
    sids
}

pub fn parse_walk_seq_update_tables(
    data: &[u8],
    graph_storage: &GraphStorage,
    item_table: &mut ItemTable,
    exclude_table: Option<&mut ActiveTable>,
    num_path: usize,
) -> (u32, u32) {
    // later codes assumes that data is non-empty...
    if data.is_empty() {
        return (0, 0);
    }

    let items_ptr = Wrap(&mut item_table.items);
    let id_prefsum_ptr = Wrap(&mut item_table.id_prefsum);

    let mutex_vec: Vec<_> = item_table
        .items
        .iter()
        .map(|x| Arc::new(Mutex::new(x)))
        .collect();

    let mut it = data.iter();
    let end = it
        .position(|x| x == &b'\t' || x == &b'\n' || x == &b'\r')
        .unwrap();

    log::debug!("parsing walk sequences of size {}..", end);

    let bp_len = Arc::new(AtomicU32::new(0));
    // ignore first > | < so that no empty is created for 1st node
    data[1..end]
        .par_split(|&x| x == b'>' || x == b'<')
        .for_each(|node| {
            let sid = graph_storage
                .get_node_id(node)
                .unwrap_or_else(|| panic!("unknown node {}", str::from_utf8(node).unwrap()));
            let idx = (sid.0 as usize) % SIZE_T;
            if let Ok(_) = mutex_vec[idx].lock() {
                unsafe {
                    (*items_ptr.0)[idx].push(sid.0);
                    (*id_prefsum_ptr.0)[idx][num_path + 1] += 1;
                }
            }
            bp_len.fetch_add(
                graph_storage.node_len(&sid),
                std::sync::atomic::Ordering::SeqCst,
            );
        });
    let bp_len = bp_len.load(std::sync::atomic::Ordering::SeqCst);

    // compute prefix sum
    let mut num_nodes_path = 0;
    for i in 0..SIZE_T {
        num_nodes_path += item_table.id_prefsum[i][num_path + 1];
        item_table.id_prefsum[i][num_path + 1] += item_table.id_prefsum[i][num_path];
    }

    // is exclude table is given, we assume that all nodes of the path are excluded
    if let Some(ex) = exclude_table {
        log::error!("flagging nodes of path as excluded");
        for i in 0..SIZE_T {
            for j in (item_table.id_prefsum[i][num_path] as usize)
                ..(item_table.id_prefsum[i][num_path + 1] as usize)
            {
                ex.items[item_table.items[i][j] as usize] |= true;
            }
        }
    }

    log::debug!("..done");
    (num_nodes_path as u32, bp_len)
}

pub fn parse_path_seq_to_item_vec(
    data: &[u8],
    graph_storage: &GraphStorage,
) -> Vec<(ItemId, Orientation)> {
    let mut it = data.iter();
    let end = it
        .position(|x| x == &b'\t' || x == &b'\n' || x == &b'\r')
        .unwrap();

    log::debug!("parsing path sequences of size {}..", end);

    let sids: Vec<(ItemId, Orientation)> = data[..end]
        .par_split(|&x| x == b',')
        .map(|node| {
            // Parallel
            let sid = graph_storage
                .get_node_id(&node[..node.len() - 1])
                .unwrap_or_else(|| {
                    panic!(
                        "unknown node {}",
                        str::from_utf8(&node[..node.len() - 1]).unwrap()
                    )
                });
            (sid, Orientation::from_pm(node[node.len() - 1]))
        })
        .collect();

    log::debug!("..done");

    sids
}

pub fn parse_path_seq_update_tables(
    data: &[u8],
    graph_storage: &GraphStorage,
    item_table: &mut ItemTable,
    exclude_table: Option<&mut ActiveTable>,
    num_path: usize,
) -> (u32, u32) {
    let mut it = data.iter();
    let end = it
        .position(|x| x == &b'\t' || x == &b'\n' || x == &b'\r')
        .unwrap();

    log::debug!("parsing path sequences of size {} bytes..", end);

    let items_ptr = Wrap(&mut item_table.items);
    let id_prefsum_ptr = Wrap(&mut item_table.id_prefsum);

    let mutex_vec: Vec<_> = item_table
        .items
        .iter()
        .map(|x| Arc::new(Mutex::new(x)))
        .collect();

    //let mut plus_strands: Vec<u32> = vec![0; rayon::current_num_threads()];
    let bp_len = data[..end]
        .par_split(|&x| x == b',')
        .map(|node| {
            let segment_id = graph_storage
                .get_node_id(&node[0..node.len() - 1])
                .unwrap_or_else(|| panic!("unknown node {}", str::from_utf8(node).unwrap()));
            // TODO: Is orientation really necessary?
            let orientation = node[node.len() - 1];
            assert!(
                orientation == b'-' || orientation == b'+',
                "unknown orientation of segment {}",
                str::from_utf8(node).unwrap()
            );
            //plus_strands[rayon::current_thread_index().unwrap()] += (orientation == b'+') as u32;

            let idx = (segment_id.0 as usize) % SIZE_T;

            if let Ok(_) = mutex_vec[idx].lock() {
                unsafe {
                    (*items_ptr.0)[idx].push(segment_id.0);
                    (*id_prefsum_ptr.0)[idx][num_path + 1] += 1;
                }
            }
            graph_storage.node_len(&segment_id)
        })
        .sum();

    // compute prefix sum
    let mut num_nodes_path = 0;
    for i in 0..SIZE_T {
        num_nodes_path += item_table.id_prefsum[i][num_path + 1];
        item_table.id_prefsum[i][num_path + 1] += item_table.id_prefsum[i][num_path];
    }

    // is exclude table is given, we assume that all nodes of the path are excluded
    if let Some(ex) = exclude_table {
        log::debug!("flagging nodes of path as excluded");
        for i in 0..SIZE_T {
            for j in (item_table.id_prefsum[i][num_path] as usize)
                ..(item_table.id_prefsum[i][num_path + 1] as usize)
            {
                ex.items[item_table.items[i][j] as usize] |= true;
            }
        }
    }

    log::debug!("..done");
    (num_nodes_path as u32, bp_len)
}
