/* standard use */
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::io::{Error, ErrorKind};
use std::str::{self, FromStr};

/* external use */
use flate2::read::MultiGzDecoder;
use quick_csv::Csv;
use rayon::prelude::*;
use strum_macros::{EnumString, EnumVariantNames};

/* internal use */
use crate::graph_broker::{AbacusByGroup, PathSegment, ThresholdContainer};
use crate::util::*;

#[derive(Debug, Clone, Copy, PartialEq, EnumString, EnumVariantNames)]
#[strum(serialize_all = "lowercase")]
pub enum OutputFormat {
    Table,
    Html,
}

pub fn bufreader_from_compressed_gfa(gfa_file: &str) -> BufReader<Box<dyn Read>> {
    log::info!("loading graph from {}", &gfa_file);
    let f = std::fs::File::open(gfa_file).expect("Error opening file");
    let reader: Box<dyn Read> = if gfa_file.ends_with(".gz") {
        log::info!("assuming that {} is gzip compressed..", &gfa_file);
        Box::new(MultiGzDecoder::new(f))
    } else {
        Box::new(f)
    };
    BufReader::new(reader)
}

pub fn parse_bed_to_path_segments<R: Read>(
    data: &mut BufReader<R>,
    use_block_info: bool,
) -> Vec<PathSegment> {
    // based on https://en.wikipedia.org/wiki/BED_(file_format)
    let mut segments = Vec::new();

    for (i, line) in data.lines().enumerate() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                panic!("error reading line {}: {}", i + 1, e);
            }
        };

        let fields = {
            let mut fields: Vec<&str> = line.split('\t').collect();
            if fields.is_empty() {
                fields = vec![&line];
            }
            fields
        };
        let path_name = fields[0];

        if path_name.starts_with("browser ")
            || path_name.starts_with("track ")
            || path_name.starts_with('#')
        {
            continue;
        }

        if fields.len() == 1 {
            segments.push(PathSegment::from_str(path_name));
        } else if fields.len() >= 3 {
            let start = usize::from_str(fields[1]).expect(&format!(
                "error line {}: `{}` is not an usize",
                i + 1,
                fields[1]
            ));
            let end = usize::from_str(fields[2]).expect(&format!(
                "error line {}: `{}` is not an usize",
                i + 1,
                fields[2]
            ));

            if use_block_info && fields.len() == 12 {
                let block_count = fields[9].parse::<usize>().unwrap_or(0);
                let block_sizes: Vec<usize> = fields[10]
                    .split(',')
                    .filter_map(|s| usize::from_str(s.trim()).ok())
                    .collect();
                let block_starts: Vec<usize> = fields[11]
                    .split(',')
                    .filter_map(|s| usize::from_str(s.trim()).ok())
                    .collect();

                if block_count == block_sizes.len() && block_count == block_starts.len() {
                    for (size, start_offset) in block_sizes.iter().zip(block_starts.iter()) {
                        let block_start = start + start_offset;
                        let block_end = block_start + size;
                        segments.push(PathSegment::from_str_start_end(
                            path_name,
                            block_start,
                            block_end,
                        ));
                    }
                } else {
                    panic!(
                        "error in block sizes/starts in line {}: counts do not match",
                        i + 1
                    );
                }
            } else {
                segments.push(PathSegment::from_str_start_end(path_name, start, end));
            }
        } else {
            panic!(
                "error in line {}: row must have either 1, 3, or 12 columns, but has 2",
                i + 1
            );
        }
    }

    segments
}

pub fn parse_groups<R: Read>(data: &mut BufReader<R>) -> Result<Vec<(PathSegment, String)>, Error> {
    let mut res: Vec<(PathSegment, String)> = Vec::new();

    let mut i = 1;
    let mut buf = vec![];
    while data.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
        //Remove new line at the end
        if let Some(&last_byte) = buf.last() {
            if last_byte == b'\n' || last_byte == b'\r' {
                buf.pop();
            }
        }
        let line = String::from_utf8(buf.clone())
            .expect(&format!("error in line {}: some character is not UTF-8", i));
        let columns: Vec<&str> = line.split('\t').collect();

        if columns.len() != 2 {
            let msg = format!("error in line {}: table must have exactly two columns", i);
            log::error!("{}", &msg);
            return Err(Error::new(ErrorKind::InvalidData, msg));
        }

        let path_seg = PathSegment::from_str(columns[0]);
        res.push((path_seg, columns[1].to_string()));

        i += 1;
        buf.clear();
    }

    Ok(res)
}

pub fn parse_tsv<R: Read>(
    data: &mut BufReader<R>,
) -> Result<(Vec<Vec<u8>>, Vec<Vec<Vec<u8>>>), Error> {
    let mut comments = Vec::new();
    let mut table = Vec::new();
    let reader = Csv::from_reader(data)
        .delimiter(b'\t')
        .flexible(true)
        .has_header(false);

    let mut is_header = true;
    for (i, row) in reader.enumerate() {
        let row = row.map_err(|_| {
            let msg = format!("unable to parse row {}", i);
            log::error!("{}", &msg);
            Error::new(ErrorKind::Other, msg)
        })?;
        let row: Vec<Vec<u8>> = row.bytes_columns().map(|x| x.to_vec()).collect();
        if row.is_empty() {
            log::info!("Empty row, skipping");
            continue;
        }
        // Push header
        if is_header && (b'#' == row[0][0]) {
            let mut c = row[0].to_vec();
            for e in &row[1..] {
                c.push(b'\t');
                c.extend(e);
            }
            comments.push(c);
        // Skip empty lines (still need to have appropriate amount of tabs)
        } else if row
            .iter()
            .map(|x| x.is_empty())
            .fold(true, |acc, x| acc && x)
        {
            log::debug!("Skipping empty line");
            continue;
        // Handle comments
        } else if b'#' == row[0][0] {
            log::debug!("Handling comment");
            let mut c = row[0].to_vec();
            for e in &row[1..] {
                c.push(b'\t');
                c.extend(e);
            }
            comments.push(c);
        // Push everything else
        } else {
            is_header = false;
            table.push(row);
        }
    }
    Ok((comments, table))
}

fn transpose_table(table: &Vec<Vec<Vec<u8>>>) -> Vec<Vec<&[u8]>> {
    let n = table.first().unwrap_or(&Vec::new()).len();

    let mut res = vec![vec![&table[0][0][..]; table.len()]; n];

    for j in 0..n {
        for i in 0..table.len() {
            res[j][i] = &table[i][j][..];
        }
    }

    res
}

fn parse_column(col: &Vec<&[u8]>, offset: usize) -> Result<Vec<usize>, Error> {
    let skip_lines = 2;
    let mut res = vec![0; col.len() - skip_lines];

    for (i, e) in col[skip_lines..].iter().enumerate() {
        if let Ok(val) = usize::from_str(str::from_utf8(e).unwrap()) {
            res[i] = val;
        } else {
            let msg = format!(
                "error in line {}: value must be integer, but is '{}'",
                i + 3 + offset,
                &str::from_utf8(e).unwrap()
            );
            log::error!("{}", &msg);
            Err(Error::new(ErrorKind::InvalidData, msg))?
        }
    }

    Ok(res)
}

pub fn parse_hists<R: Read>(
    data: &mut BufReader<R>,
) -> Result<(Vec<(CountType, Vec<usize>)>, Vec<Vec<u8>>), Error> {
    log::info!("loading coverage histogram from");
    let (comments, raw_table) = parse_tsv(data)?;
    let raw_table = transpose_table(&raw_table);
    if raw_table.len() < 4 && b"panacus" != raw_table[0][0] {
        let msg = format!(
            "error in line {}: table appears not to be generated by panacus",
            comments.len()
        );
        log::error!("{}", &msg);
        return Err(Error::new(ErrorKind::InvalidData, msg));
    }

    let mut res = Vec::new();

    let index = parse_column(&raw_table[0], comments.len())?;
    let mx = index.iter().max().unwrap();
    for col in &raw_table[1..] {
        if b"hist" == &col[0] {
            let count = CountType::from_str(str::from_utf8(col[1]).unwrap()).map_err(|_| {
                let msg = format!(
                    "error in line {}: expected count type declaration, but got '{}'",
                    2 + comments.len(),
                    &str::from_utf8(col[1]).unwrap()
                );
                log::error!("{}", &msg);
                Error::new(ErrorKind::InvalidData, msg)
            })?;
            let mut cov = vec![0; mx + 1];
            for (i, c) in index.iter().zip(parse_column(col, comments.len())?) {
                cov[*i] = c;
            }

            res.push((count, cov));
        }
    }

    if res.is_empty() {
        let msg = "table does not contain hist columns";
        log::error!("{}", msg);
        Err(Error::new(ErrorKind::InvalidData, msg))
    } else {
        Ok((res, comments))
    }
}

#[allow(dead_code)]
pub fn parse_threshold_file<R: Read>(data: &mut BufReader<R>) -> Result<Vec<Threshold>, Error> {
    let mut res = Vec::new();

    let reader = Csv::from_reader(data)
        .delimiter(b'\t')
        .flexible(true)
        .has_header(false);
    for (i, row) in reader.enumerate() {
        let row = row.unwrap();
        let mut row_it = row.bytes_columns();
        if let Some(col) = row_it.next() {
            let threshold_str = str::from_utf8(col).unwrap();
            if let Ok(t) = usize::from_str(threshold_str) {
                res.push(Threshold::Absolute(t));
            } else if let Ok(t) = f64::from_str(threshold_str) {
                res.push(Threshold::Relative(t));
            } else {
                return Err(Error::new(
                    ErrorKind::InvalidData,
                    &format!(
                        "threshold \"{}\" (line {}) is neither an integer nor a float",
                        &threshold_str,
                        i + 1
                    )[..],
                ));
            }
        }
    }

    Ok(res)
}

//#[allow(dead_code)]
//pub fn parse_graph_aux<R: Read>(
//    data: &mut BufReader<R>,
//    index_edges: bool,
//) -> Result<
//    (
//        HashMap<Vec<u8>, ItemId>,
//        Vec<ItemIdSize>,
//        Option<Vec<Vec<u8>>>,
//        Vec<PathSegment>,
//    ),
//    std::io::Error,
//> {
//    // let's start
//    // IMPORTANT: id must be > 0, otherwise counting procedure will produce errors
//    let mut node_id = 1;
//    let mut node2id: HashMap<Vec<u8>, ItemId> = HashMap::default();
//    let mut edges: Option<Vec<Vec<u8>>> = if index_edges { Some(Vec::new()) } else { None };
//    let mut path_segments: Vec<PathSegment> = Vec::new();
//    let mut node_len: Vec<ItemIdSize> = Vec::new();
//    // add empty element to node_len to make it in sync with node_id
//    node_len.push(ItemIdSize::MAX);
//
//    let mut buf = vec![];
//    let mut i = 1;
//    while data.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
//        // really really make sure that we hit a new line, which is not guaranteed when reading
//        // from a compressed buffer
//        while buf.last().unwrap() != &b'\n' {
//            if data.read_until(b'\n', &mut buf).unwrap_or(0) == 0
//                && buf.last().unwrap_or(&b' ') != &b'\n'
//            {
//                buf.push(b'\n')
//            }
//        }
//        if buf[0] == b'S' {
//            let mut iter = buf[2..].iter();
//            let offset = iter.position(|&x| x == b'\t').ok_or_else(|| {
//                std::io::Error::new(
//                    std::io::ErrorKind::InvalidData,
//                    format!(
//                        "segment in line {} stops prematurely before declaration of identifier: {}",
//                        i,
//                        str::from_utf8(&buf).unwrap()
//                    ),
//                )
//            })?;
//            if node2id
//                .insert(buf[2..offset + 2].to_vec(), ItemId(node_id))
//                .is_some()
//            {
//                return Err(std::io::Error::new(
//                    std::io::ErrorKind::InvalidData,
//                    format!(
//                        "segment with ID {} occurs multiple times in GFA",
//                        str::from_utf8(&buf[2..offset + 2]).unwrap()
//                    ),
//                ));
//            }
//            node_id += 1;
//            let offset = iter
//                .position(|&x| x == b'\t' || x == b'\n' || x == b'\r')
//                .unwrap();
//            node_len.push(offset as ItemIdSize);
//        } else if index_edges && buf[0] == b'L' {
//            edges.as_mut().unwrap().push(buf.to_vec());
//        } else if buf[0] == b'P' {
//            let (path_seg, _) = parse_path_identifier(&buf);
//            path_segments.push(path_seg);
//        } else if buf[0] == b'W' {
//            let (path_seg, _) = parse_walk_identifier(&buf);
//            path_segments.push(path_seg);
//        }
//
//        buf.clear();
//        i += 1;
//    }
//
//    Ok((node2id, node_len, edges, path_segments))
//}

// pub fn parse_cdbg_gfa_paths_walks<R: Read>(
//     data: &mut BufReader<R>,
//     abacus_aux: &GraphMask,
//     graph_aux: &GraphStorage,
//     k: usize,
// ) -> ItemTable {
//     let mut item_table = ItemTable::new(graph_aux.path_segments.len());
//     //let mut k_count = 0;
//     //let (mut subset_covered_bps, mut exclude_table, include_map, exclude_map) = abacus_aux.load_optional_subsetting(&graph_aux, &count);
//
//     let mut num_path = 0;
//     let mut buf = vec![];
//     while data.read_until(b'\n', &mut buf).unwrap_or(0) > 0 {
//         if buf[0] == b'P' {
//             let (_path_seg, buf_path_seg) = parse_path_identifier(&buf);
//             let sids = parse_path_seq_to_item_vec(&buf_path_seg, &graph_aux);
//             let mut u_sid = sids[0].0 .0 as usize - 1;
//             let mut u_ori = sids[0].1;
//             for i in 1..sids.len() {
//                 let v_sid = sids[i].0 .0 as usize - 1;
//                 let v_ori = sids[i].1;
//                 let k_plus_one_mer =
//                     graph_aux.get_k_plus_one_mer_edge(u_sid, u_ori, v_sid, v_ori, k);
//                 //println!("{}", bits2kmer(k_plus_one_mer, k+1));
//                 let infix = get_infix(k_plus_one_mer, k);
//                 let infix_rc = revcmp(infix, k - 1);
//                 if infix < infix_rc {
//                     let idx = (infix as usize) % SIZE_T;
//                     item_table.items[idx].push(k_plus_one_mer);
//                     item_table.id_prefsum[idx][num_path + 1] += 1;
//                 } else if infix > infix_rc {
//                     let idx = (infix_rc as usize) % SIZE_T;
//                     item_table.items[idx].push(revcmp(k_plus_one_mer, k + 1));
//                     item_table.id_prefsum[idx][num_path + 1] += 1;
//                 } // else ignore palindrome, since it always breaks the node
//
//                 u_sid = v_sid;
//                 u_ori = v_ori;
//             }
//
//             // compute prefix sum
//             for i in 0..SIZE_T {
//                 item_table.id_prefsum[i][num_path + 1] += item_table.id_prefsum[i][num_path];
//             }
//
//             num_path += 1;
//         }
//         buf.clear();
//     }
//
//     item_table
// }

pub fn write_table(headers: &Vec<Vec<String>>, columns: &Vec<Vec<f64>>) -> Result<String, Error> {
    let n = headers.first().unwrap_or(&Vec::new()).len();
    let mut res = String::new();
    for i in 0..n {
        for j in 0..headers.len() {
            if j > 0 {
                res.push_str("\t");
            }
            res.push_str(&format!("{:0}", headers[j][i]));
        }
        res.push_str("\n");
    }
    let n = columns.first().unwrap_or(&Vec::new()).len();
    for i in 0..n {
        res.push_str(&i.to_string());
        for j in 0..columns.len() {
            res.push_str(&format!("\t{:0}", columns[j][i].floor()));
        }
        res.push_str("\n");
    }
    Ok(res)
}

pub fn write_ordered_table<W: Write>(
    headers: &Vec<Vec<String>>,
    columns: &Vec<Vec<f64>>,
    index: &Vec<String>,
    out: &mut BufWriter<W>,
) -> Result<(), std::io::Error> {
    let n = headers.first().unwrap_or(&Vec::new()).len();

    for i in 0..n {
        for j in 0..headers.len() {
            if j > 0 {
                write!(out, "\t")?;
            }
            write!(out, "{:0}", headers[j][i])?;
        }
        writeln!(out)?;
    }
    let n = columns.first().unwrap_or(&Vec::new()).len();
    for i in 1..n {
        write!(out, "{}", index[i - 1])?;
        for column in columns {
            write!(out, "\t{:0}", column[i].floor())?;
        }
        writeln!(out)?;
    }

    Ok(())
}

// pub fn write_hist_table<W: Write>(hists: &[Hist], out: &mut BufWriter<W>) -> Result<(), Error> {
//     log::info!("reporting hist table");
//     writeln!(
//         out,
//         "# {}",
//         std::env::args().collect::<Vec<String>>().join(" ")
//     )?;
//
//     let mut header_cols = vec![vec![
//         "panacus".to_string(),
//         "count".to_string(),
//         String::new(),
//         String::new(),
//     ]];
//     let mut output_columns = Vec::new();
//     for h in hists.iter() {
//         output_columns.push(h.coverage.iter().map(|x| *x as f64).collect());
//         header_cols.push(vec![
//             "hist".to_string(),
//             h.count.to_string(),
//             String::new(),
//             String::new(),
//         ])
//     }
//     write_table(&header_cols, &output_columns, out)
// }
fn write_metadata_comments<W: Write>(out: &mut BufWriter<W>) -> Result<(), Error> {
    writeln!(
        out,
        "# {}",
        std::env::args().collect::<Vec<String>>().join(" ")
    )?;
    let version = option_env!("GIT_HASH").unwrap_or(env!("CARGO_PKG_VERSION"));
    writeln!(out, "# version {}", version)
}

pub fn write_ordered_histgrowth_table<W: Write>(
    abacus_group: &AbacusByGroup,
    hist_aux: &ThresholdContainer,
    node_lens: &Vec<u32>,
    out: &mut BufWriter<W>,
) -> Result<(), Error> {
    log::info!("reporting ordered-growth table");
    write_metadata_comments(out)?;

    let mut output_columns: Vec<Vec<f64>> = hist_aux
        .coverage
        .par_iter()
        .zip(&hist_aux.quorum)
        .map(|(c, q)| {
            log::info!(
                "calculating ordered growth for coverage >= {} and quorum >= {}",
                &c,
                &q
            );
            abacus_group.calc_growth(c, q, node_lens)
        })
        .collect();

    // insert empty row for 0 element
    for c in &mut output_columns {
        c.insert(0, f64::NAN);
    }
    let m = hist_aux.coverage.len();
    let mut header_cols = vec![vec![
        "panacus".to_string(),
        "count".to_string(),
        "coverage".to_string(),
        "quorum".to_string(),
    ]];
    header_cols.extend(
        std::iter::repeat("ordered-growth")
            .take(m)
            .zip(std::iter::repeat(abacus_group.count).take(m))
            .zip(hist_aux.coverage.iter())
            .zip(&hist_aux.quorum)
            .map(|(((p, t), c), q)| {
                vec![p.to_string(), t.to_string(), c.get_string(), q.get_string()]
            })
            .collect::<Vec<Vec<String>>>(),
    );
    write_ordered_table(&header_cols, &output_columns, &abacus_group.groups, out)
}

#[cfg(test)]
mod tests {
    //use super::*;
    //use std::collections::HashMap;
    //use std::io::Cursor;
    //use std::str::from_utf8;

    //fn mock_graph_auxilliary() -> GraphStorage {
    //    GraphStorage {
    //        node2id: {
    //            let mut node2id = HashMap::new();
    //            node2id.insert(b"node1".to_vec(), ItemId(1));
    //            node2id.insert(b"node2".to_vec(), ItemId(2));
    //            node2id.insert(b"node3".to_vec(), ItemId(3));
    //            node2id
    //        },
    //        node_lens: Vec::new(),
    //        edge2id: None,
    //        path_segments: Vec::new(),
    //        node_count: 3,
    //        edge_count: 0,
    //        degree: Some(Vec::new()),
    //        //extremities: Some(Vec::new())
    //    }
    // use super::*;
    // use std::collections::HashMap;
    // use std::io::Cursor;
    // use std::str::from_utf8;

    // fn mock_graph_auxilliary() -> GraphStorage {
    //     GraphStorage {
    //         node2id: {
    //             let mut node2id = HashMap::new();
    //             node2id.insert(b"node1".to_vec(), ItemId(1));
    //             node2id.insert(b"node2".to_vec(), ItemId(2));
    //             node2id.insert(b"node3".to_vec(), ItemId(3));
    //             node2id
    //         },
    //         node_lens: Vec::new(),
    //         edge2id: None,
    //         path_segments: Vec::new(),
    //         node_count: 3,
    //         edge_count: 0,
    //         degree: Some(Vec::new()),
    //         //extremities: Some(Vec::new())
    //     }
    // }

    // // Test parse_walk_identifier function
    // #[test]
    // fn test_parse_walk_identifier() {
    //     let data = b"W\tG01\t0\tU00096.3\t3\t4641652\t>3>4>5>7>8>";
    //     let (path_segment, data) = parse_walk_identifier(data);
    //     dbg!(&path_segment);

    //     assert_eq!(path_segment.sample, "G01".to_string());
    //     assert_eq!(path_segment.haplotype, Some("0".to_string()));
    //     assert_eq!(path_segment.seqid, Some("U00096.3".to_string()));
    //     assert_eq!(path_segment.start, Some(3));
    //     assert_eq!(path_segment.end, Some(4641652));
    //     assert_eq!(from_utf8(data).unwrap(), ">3>4>5>7>8>");
    // }

    // #[test]
    // #[should_panic(expected = "unwrap")]
    // fn test_parse_walk_identifier_invalid_utf8() {
    //     let data = b"W\tG01\t0\tU00096.3\t3\t>3>4>5>7>8>";
    //     parse_walk_identifier(data);
    // }

    // // Test parse_path_identifier function
    // #[test]
    // fn test_parse_path_identifier() {
    //     let data = b"P\tGCF_000005845.2_ASM584v2_genomic.fna#0#contig1\t1+,2+,3+,4+\t*";
    //     let (path_segment, rest) = parse_path_identifier(data);

    //     assert_eq!(
    //         path_segment.to_string(),
    //         "GCF_000005845.2_ASM584v2_genomic.fna#0#contig1"
    //     );
    //     assert_eq!(from_utf8(rest).unwrap(), "1+,2+,3+,4+\t*");
    // }

    //// Test parse_walk_seq_to_item_vec function
    //#[test]
    //fn test_parse_walk_seq_to_item_vec() {
    //    let data = b">node1<node2\t";
    //    let graph_aux = MockGraphStorage::new();

    //    let result = parse_walk_seq_to_item_vec(data, &graph_aux);
    //    assert_eq!(result.len(), 2);
    //    assert_eq!(result[0], (1, Orientation::Forward));
    //    assert_eq!(result[1], (2, Orientation::Backward));
    //}
    //
    //// Test parse_walk_identifier function
    //#[test]
    //fn test_parse_walk_identifier() {
    //    let data = b"W\tG01\t0\tU00096.3\t3\t4641652\t>3>4>5>7>8>";
    //    let (path_segment, data) = parse_walk_identifier(data);
    //    dbg!(&path_segment);
    //
    //    assert_eq!(path_segment.sample, "G01".to_string());
    //    assert_eq!(path_segment.haplotype, Some("0".to_string()));
    //    assert_eq!(path_segment.seqid, Some("U00096.3".to_string()));
    //    assert_eq!(path_segment.start, Some(3));
    //    assert_eq!(path_segment.end, Some(4641652));
    //    assert_eq!(from_utf8(data).unwrap(), ">3>4>5>7>8>");
    //}
    //
    //#[test]
    //#[should_panic(expected = "unwrap")]
    //fn test_parse_walk_identifier_invalid_utf8() {
    //    let data = b"W\tG01\t0\tU00096.3\t3\t>3>4>5>7>8>";
    //    parse_walk_identifier(data);
    //}
    //
    //// Test parse_path_identifier function
    //#[test]
    //fn test_parse_path_identifier() {
    //    let data = b"P\tGCF_000005845.2_ASM584v2_genomic.fna#0#contig1\t1+,2+,3+,4+\t*";
    //    let (path_segment, rest) = parse_path_identifier(data);
    //
    //    assert_eq!(
    //        path_segment.to_string(),
    //        "GCF_000005845.2_ASM584v2_genomic.fna#0#contig1"
    //    );
    //    assert_eq!(from_utf8(rest).unwrap(), "1+,2+,3+,4+\t*");
    //}
    //
    ////// Test parse_walk_seq_to_item_vec function
    ////#[test]
    ////fn test_parse_walk_seq_to_item_vec() {
    ////    let data = b">node1<node2\t";
    ////    let graph_aux = MockGraphStorage::new();
    //
    ////    let result = parse_walk_seq_to_item_vec(data, &graph_aux);
    ////    assert_eq!(result.len(), 2);
    ////    assert_eq!(result[0], (1, Orientation::Forward));
    ////    assert_eq!(result[1], (2, Orientation::Backward));
    ////}
    //
    //// Test parse_path_seq_to_item_vec function
    //#[test]
    //fn test_parse_path_seq_to_item_vec() {
    //    let data = b"node1+,node2-\t*";
    //    let graph_aux = mock_graph_auxilliary();
    //
    //    let result = parse_path_seq_to_item_vec(data, &graph_aux);
    //    assert_eq!(result.len(), 2);
    //    assert_eq!(result[0], (ItemId(1), Orientation::Forward));
    //    assert_eq!(result[1], (ItemId(2), Orientation::Backward));
    //}
    //
    ////#[test]
    ////fn test_parse_cdbg_gfa_paths_walks() {
    ////    let data = b"P\tpath1\tnode1+,node2-\n";
    ////    let mut reader = BufReader::new(&data[..]);
    ////    let graph_aux = mock_graph_auxilliary();
    //
    ////    let result = parse_cdbg_gfa_paths_walks(&mut reader, &graph_aux, 3);
    ////    dbg!(&result);
    //
    ////    assert_eq!(result.items.len(), SIZE_T);
    ////}
    //
    //// Test update_tables
    ////#[test]
    ////fn test_update_tables() {
    ////    let mut item_table = ItemTable::new(10);
    ////    let graph_aux = mock_graph_auxilliary();
    ////    let path = vec![(1, Orientation::Forward), (2, Orientation::Backward)];
    ////    let include_coords = &[];
    ////    let exclude_coords = &[];
    ////    let offset = 0;
    //
    ////    update_tables(
    ////        &mut item_table,
    ////        &mut None,
    ////        &mut None,
    ////        0,
    ////        &graph_aux,
    ////        path,
    ////        include_coords,
    ////        exclude_coords,
    ////        offset,
    ////    );
    //
    ////    dbg!(&item_table);
    ////    assert!(item_table.items[1].contains(&1));
    ////    assert!(item_table.items[2].contains(&2));
    ////}
    //
    //// parse_bed_to_path_segments testing
    //#[test]
    //fn test_parse_bed_with_1_column() {
    //    let bed_data = b"chr1\nchr2";
    //    let mut reader = BufReader::new(Cursor::new(bed_data));
    //    let result = parse_bed_to_path_segments(&mut reader, true);
    //    assert_eq!(
    //        result,
    //        vec![PathSegment::from_str("chr1"), PathSegment::from_str("chr2"),]
    //    );
    //}
    //
    //#[test]
    //#[should_panic(
    //    expected = "error in line 1: row must have either 1, 3, or 12 columns, but has 2"
    //)]
    //fn test_parse_bed_with_2_columns() {
    //    let bed_data = b"chr1\t1000\n";
    //    let mut reader = BufReader::new(Cursor::new(bed_data));
    //    parse_bed_to_path_segments(&mut reader, false);
    //}
    //
    //#[test]
    //#[should_panic(expected = "error line 1: `100.5` is not an usize")]
    //fn test_parse_bed_with_2_columns_no_usize() {
    //    let bed_data = b"chr1\t100.5\tACGT\n";
    //    let mut reader = BufReader::new(Cursor::new(bed_data));
    //    parse_bed_to_path_segments(&mut reader, false);
    //}
    //
    //#[test]
    //fn test_parse_bed_with_3_columns() {
    //    let bed_data = b"chr1\t1000\t2000\nchr2\t1500\t2500";
    //    let mut reader = BufReader::new(Cursor::new(bed_data));
    //    let result = parse_bed_to_path_segments(&mut reader, false);
    //    assert_eq!(
    //        result,
    //        vec![
    //            {
    //                let mut tmp = PathSegment::from_str("chr1");
    //                tmp.start = Some(1000);
    //                tmp.end = Some(2000);
    //                tmp
    //            },
    //            {
    //                let mut tmp = PathSegment::from_str("chr2");
    //                tmp.start = Some(1500);
    //                tmp.end = Some(2500);
    //                tmp
    //            }
    //        ]
    //    );
    //}
    //
    //#[test]
    //fn test_parse_bed_with_12_columns_no_block() {
    //    let bed_data = b"chr1\t1000\t2000\tname\t0\t+\t1000\t2000\t0\t2\t100,100\t0,900\n";
    //    let mut reader = BufReader::new(Cursor::new(bed_data));
    //    let result = parse_bed_to_path_segments(&mut reader, false);
    //    assert_eq!(
    //        result,
    //        vec![{
    //            let mut tmp = PathSegment::from_str("chr1");
    //            tmp.start = Some(1000);
    //            tmp.end = Some(2000);
    //            tmp
    //        }]
    //    );
    //}
    //
    //#[test]
    //fn test_parse_bed_with_12_columns_with_block() {
    //    let bed_data = b"chr1\t1000\t2000\tname\t0\t+\t1000\t2000\t0\t2\t100,100\t0,900\n";
    //    let mut reader = BufReader::new(Cursor::new(bed_data));
    //    let result = parse_bed_to_path_segments(&mut reader, true);
    //    assert_eq!(
    //        result,
    //        vec![
    //            {
    //                let mut tmp = PathSegment::from_str("chr1");
    //                tmp.start = Some(1000);
    //                tmp.end = Some(1100);
    //                tmp
    //            },
    //            {
    //                let mut tmp = PathSegment::from_str("chr1");
    //                tmp.start = Some(1900);
    //                tmp.end = Some(2000);
    //                tmp
    //            }
    //        ]
    //    );
    //}
    //
    //#[test]
    //fn test_parse_bed_with_header() {
    //    let bed_data = b"browser position chr1:1-1000\nbrowser position chr7:127471196-127495720\nbrowser hide all\ntrack name='ItemRGBDemo' description='Item RGB demonstration' visibility=2 itemRgb='On'\nchr1\t1000\t2000\nchr2\t1500\t2500\n";
    //    let mut reader = BufReader::new(Cursor::new(bed_data));
    //    let result = parse_bed_to_path_segments(&mut reader, false);
    //    assert_eq!(
    //        result,
    //        vec![
    //            {
    //                let mut tmp = PathSegment::from_str("chr1");
    //                tmp.start = Some(1000);
    //                tmp.end = Some(2000);
    //                tmp
    //            },
    //            {
    //                let mut tmp = PathSegment::from_str("chr2");
    //                tmp.start = Some(1500);
    //                tmp.end = Some(2500);
    //                tmp
    //            }
    //        ]
    //    );
    //}
    //
    //#[test]
    //fn test_parse_groups_with_valid_input() {
    //    //let (graph_aux, _, _) = setup_test_data();
    //    let file_name = "test/test_groups.txt";
    //    let test_path_segments = vec![
    //        PathSegment::from_str("a#0"),
    //        PathSegment::from_str("b#0"),
    //        PathSegment::from_str("c#0"),
    //        PathSegment::from_str("c#1"),
    //        PathSegment::from_str("d#0"),
    //    ];
    //    let test_groups = vec!["G1", "G1", "G2", "G2", "G2"];
    //
    //    let mut data = BufReader::new(std::fs::File::open(file_name).unwrap());
    //    let result = parse_groups(&mut data);
    //    assert!(result.is_ok(), "Expected successful group loading");
    //    let path_segments_group = result.unwrap();
    //    assert!(
    //        path_segments_group.len() > 0,
    //        "Expected non-empty group assignments"
    //    );
    //    assert_eq!(path_segments_group.len(), 5); // number of paths == groups
    //    for (i, (path_seg, group)) in path_segments_group.into_iter().enumerate() {
    //        assert_eq!(path_seg, test_path_segments[i]);
    //        assert_eq!(group, test_groups[i]);
    //    }
    //}

    // parse_bed_to_path_segments testing
    // #[test]
    // fn test_parse_bed_with_1_column() {
    //     let bed_data = b"chr1\nchr2";
    //     let mut reader = BufReader::new(Cursor::new(bed_data));
    //     let result = parse_bed_to_path_segments(&mut reader, true);
    //     assert_eq!(
    //         result,
    //         vec![PathSegment::from_str("chr1"), PathSegment::from_str("chr2"),]
    //     );
    // }

    // #[test]
    // #[should_panic(
    //     expected = "error in line 1: row must have either 1, 3, or 12 columns, but has 2"
    // )]
    // fn test_parse_bed_with_2_columns() {
    //     let bed_data = b"chr1\t1000\n";
    //     let mut reader = BufReader::new(Cursor::new(bed_data));
    //     parse_bed_to_path_segments(&mut reader, false);
    // }

    // #[test]
    // #[should_panic(expected = "error line 1: `100.5` is not an usize")]
    // fn test_parse_bed_with_2_columns_no_usize() {
    //     let bed_data = b"chr1\t100.5\tACGT\n";
    //     let mut reader = BufReader::new(Cursor::new(bed_data));
    //     parse_bed_to_path_segments(&mut reader, false);
    // }

    // #[test]
    // fn test_parse_bed_with_3_columns() {
    //     let bed_data = b"chr1\t1000\t2000\nchr2\t1500\t2500";
    //     let mut reader = BufReader::new(Cursor::new(bed_data));
    //     let result = parse_bed_to_path_segments(&mut reader, false);
    //     assert_eq!(
    //         result,
    //         vec![
    //             {
    //                 let mut tmp = PathSegment::from_str("chr1");
    //                 tmp.start = Some(1000);
    //                 tmp.end = Some(2000);
    //                 tmp
    //             },
    //             {
    //                 let mut tmp = PathSegment::from_str("chr2");
    //                 tmp.start = Some(1500);
    //                 tmp.end = Some(2500);
    //                 tmp
    //             }
    //         ]
    //     );
    // }

    // #[test]
    // fn test_parse_bed_with_12_columns_no_block() {
    //     let bed_data = b"chr1\t1000\t2000\tname\t0\t+\t1000\t2000\t0\t2\t100,100\t0,900\n";
    //     let mut reader = BufReader::new(Cursor::new(bed_data));
    //     let result = parse_bed_to_path_segments(&mut reader, false);
    //     assert_eq!(
    //         result,
    //         vec![{
    //             let mut tmp = PathSegment::from_str("chr1");
    //             tmp.start = Some(1000);
    //             tmp.end = Some(2000);
    //             tmp
    //         }]
    //     );
    // }

    // #[test]
    // fn test_parse_bed_with_12_columns_with_block() {
    //     let bed_data = b"chr1\t1000\t2000\tname\t0\t+\t1000\t2000\t0\t2\t100,100\t0,900\n";
    //     let mut reader = BufReader::new(Cursor::new(bed_data));
    //     let result = parse_bed_to_path_segments(&mut reader, true);
    //     assert_eq!(
    //         result,
    //         vec![
    //             {
    //                 let mut tmp = PathSegment::from_str("chr1");
    //                 tmp.start = Some(1000);
    //                 tmp.end = Some(1100);
    //                 tmp
    //             },
    //             {
    //                 let mut tmp = PathSegment::from_str("chr1");
    //                 tmp.start = Some(1900);
    //                 tmp.end = Some(2000);
    //                 tmp
    //             }
    //         ]
    //     );
    // }

    // #[test]
    // fn test_parse_bed_with_header() {
    //     let bed_data = b"browser position chr1:1-1000\nbrowser position chr7:127471196-127495720\nbrowser hide all\ntrack name='ItemRGBDemo' description='Item RGB demonstration' visibility=2 itemRgb='On'\nchr1\t1000\t2000\nchr2\t1500\t2500\n";
    //     let mut reader = BufReader::new(Cursor::new(bed_data));
    //     let result = parse_bed_to_path_segments(&mut reader, false);
    //     assert_eq!(
    //         result,
    //         vec![
    //             {
    //                 let mut tmp = PathSegment::from_str("chr1");
    //                 tmp.start = Some(1000);
    //                 tmp.end = Some(2000);
    //                 tmp
    //             },
    //             {
    //                 let mut tmp = PathSegment::from_str("chr2");
    //                 tmp.start = Some(1500);
    //                 tmp.end = Some(2500);
    //                 tmp
    //             }
    //         ]
    //     );
    // }

    // #[test]
    // fn test_parse_groups_with_valid_input() {
    //     //let (graph_aux, _, _) = setup_test_data();
    //     let file_name = "test/test_groups.txt";
    //     let test_path_segments = vec![
    //         PathSegment::from_str("a#0"),
    //         PathSegment::from_str("b#0"),
    //         PathSegment::from_str("c#0"),
    //         PathSegment::from_str("c#1"),
    //         PathSegment::from_str("d#0"),
    //     ];
    //     let test_groups = vec!["G1", "G1", "G2", "G2", "G2"];

    //     let mut data = BufReader::new(std::fs::File::open(file_name).unwrap());
    //     let result = parse_groups(&mut data);
    //     assert!(result.is_ok(), "Expected successful group loading");
    //     let path_segments_group = result.unwrap();
    //     assert!(
    //         path_segments_group.len() > 0,
    //         "Expected non-empty group assignments"
    //     );
    //     assert_eq!(path_segments_group.len(), 5); // number of paths == groups
    //     for (i, (path_seg, group)) in path_segments_group.into_iter().enumerate() {
    //         assert_eq!(path_seg, test_path_segments[i]);
    //         assert_eq!(group, test_groups[i]);
    //     }
    // }
}
