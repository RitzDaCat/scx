// SPDX-License-Identifier: GPL-2.0
use crate::{Topology, CoreType, NR_CPU_IDS};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Powermode {
    Turbo,
    Performance,
    Powersave,
    Any,
}

pub fn get_primary_cpus(mode: Powermode) -> std::io::Result<Vec<usize>> {
    let cpus: Vec<usize> = Topology::new()
        .unwrap()
        .all_cores
        .values()
        .flat_map(|core| &core.cpus)
        .filter_map(|(cpu_id, cpu)| match (&mode, &cpu.core_type) {
            (Powermode::Turbo, CoreType::Big { turbo: true }) |
            (Powermode::Performance, CoreType::Big { .. }) |
            (Powermode::Powersave, CoreType::Little) => Some(*cpu_id),
            (Powermode::Any, ..) => Some(*cpu_id),
            _ => None,
        })
        .collect();
    Ok(cpus)
}

pub fn parse_cpu_list(optarg: &str) -> Result<Vec<usize>, String> {
    let mut cpus = Vec::new();
    let mut seen = HashSet::new();

    // Handle special keywords
    if let Some(mode) = match optarg {
        "powersave" => Some(Powermode::Powersave),
        "performance" => Some(Powermode::Performance),
        "turbo" => Some(Powermode::Turbo),
        "all" => Some(Powermode::Any),
        _ => None,
    } {
        return get_primary_cpus(mode).map_err(|e| e.to_string());
    }

    if optarg
        .chars()
        .any(|c| !c.is_ascii_digit() && c != '-' && c != ',' && !c.is_whitespace())
    {
        return Err("Invalid character in CPU list".to_string());
    }

    let cleaned = optarg.replace(' ', "\t");

    for token in cleaned.split(',') {
        let token = token.trim_matches(|c: char| c.is_whitespace());
        if token.is_empty() { continue; }

        if let Some((start_str, end_str)) = token.split_once('-') {
            let start = start_str.trim().parse::<usize>().map_err(|_| "Invalid range start")?;
            let end = end_str.trim().parse::<usize>().map_err(|_| "Invalid range end")?;
            if start > end { return Err(format!("Invalid CPU range: {}-{}", start, end)); }
            for i in start..=end {
                if cpus.len() >= *NR_CPU_IDS {
                    return Err(format!("Too many CPUs specified (max {})", *NR_CPU_IDS));
                }
                if seen.insert(i) { cpus.push(i); }
            }
        } else {
            let cpu = token.parse::<usize>().map_err(|_| format!("Invalid CPU: {}", token))?;
            if cpus.len() >= *NR_CPU_IDS {
                return Err(format!("Too many CPUs specified (max {})", *NR_CPU_IDS));
            }
            if seen.insert(cpu) { cpus.push(cpu); }
        }
    }

    Ok(cpus)
}

#[cfg(test)]
mod tests {
    use super::parse_cpu_list;

    #[test]
    fn parses_single_cpu() {
        let v = parse_cpu_list("3").unwrap();
        assert_eq!(v, vec![3]);
    }

    #[test]
    fn parses_range_and_single() {
        let mut v = parse_cpu_list("0-2,5").unwrap();
        v.sort();
        assert_eq!(v, vec![0,1,2,5]);
    }

    #[test]
    fn rejects_bad_chars() {
        assert!(parse_cpu_list("0-2,a").is_err());
    }
}
