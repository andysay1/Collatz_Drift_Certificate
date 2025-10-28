use assert_cmd::prelude::*;
use std::process::Command;
use tempfile::tempdir;
use std::fs::File;
use std::io::{Write, Read};

fn collatz_s_sum(k: u32, l: u32, idx: usize) -> u32 {
    let mask: u64 = (1u64 << k) - 1;
    let mut m = ((idx as u64) << 1) | 1;
    let mut s: u64 = 0;
    for _ in 0..l {
        let t = 3u64.wrapping_mul(m & mask).wrapping_add(1);
        let e = t.trailing_zeros() as u64;
        s += e;
        m = (t >> e) & mask;
    }
    s.min(u32::MAX as u64) as u32
}

#[test]
fn gen_v2_and_verify_roundtrip_small() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempdir()?;
    let dir_path = dir.path();

    // Generate v2 with defaults
    let mut cmd = Command::cargo_bin("collatz_cert")?;
    cmd.current_dir(dir_path)
        .args(["gen", "--k", "4", "--l", "8", "--threads", "2"])
        .assert()
        .success();

    // Verify produced files
    let mut cmd2 = Command::cargo_bin("collatz_cert")?;
    cmd2.current_dir(dir_path)
        .args([
            "verify", "--k", "4", "--l", "8",
            "--table", "table_k4_l8_v2.bin",
            "--manifest", "cert_k4_l8_v2.json",
            "--threads", "2",
        ])
        .assert()
        .success();

    // Check manifest has file_ver=2
    let mut s = String::new();
    File::open(dir_path.join("cert_k4_l8_v2.json"))?.read_to_string(&mut s)?;
    let v: serde_json::Value = serde_json::from_str(&s)?;
    assert_eq!(v["file_ver"].as_u64().unwrap_or(0), 2);
    Ok(())
}

#[test]
fn verify_v1_synthetic_small() -> Result<(), Box<dyn std::error::Error>> {
    // Build synthetic v1 file for k=4, l=8
    let k: u32 = 4;
    let l: u32 = 8;
    let count: usize = 1usize << (k as usize - 1);

    let dir = tempdir()?;
    let dir_path = dir.path();

    // Prepare table (u16) and header+trailer
    let mut table_bytes: Vec<u8> = Vec::with_capacity(count * 2);
    let mut min_s = u32::MAX;
    for idx in 0..count {
        let s_i = collatz_s_sum(k, l, idx);
        min_s = min_s.min(s_i);
        let v = (s_i as u16).to_le_bytes();
        table_bytes.extend_from_slice(&v);
    }

    // Header: magic, ver=1, k, l, count, reserved
    let mut file_bytes: Vec<u8> = Vec::new();
    file_bytes.extend_from_slice(b"CALT");
    file_bytes.extend_from_slice(1u32.to_le_bytes().as_slice());
    file_bytes.extend_from_slice(k.to_le_bytes().as_slice());
    file_bytes.extend_from_slice(l.to_le_bytes().as_slice());
    file_bytes.extend_from_slice((count as u64).to_le_bytes().as_slice());
    file_bytes.extend_from_slice(&[0u8; 8]);
    file_bytes.extend_from_slice(&table_bytes);

    // Trailer: sha256(table_bytes)
    let mut hasher = sha2::Sha256::new();
    use sha2::Digest;
    hasher.update(&table_bytes);
    let digest = hasher.finalize();
    file_bytes.extend_from_slice(&digest);

    // Write table file
    let table_path = dir_path.join("table_v1_k4_l8.bin");
    let mut f = File::create(&table_path)?;
    f.write_all(&file_bytes)?;

    // Manifest JSON with required fields
    let thr = ((l as f64) * (3f64.log2())).floor() as u32 + 1;
    let pass = (min_s as u32) >= thr;
    let eps = (min_s as f64) / (l as f64) - 3f64.log2();
    let mut hex = String::new();
    for b in digest.as_slice() { hex.push_str(&format!("{:02x}", b)); }
    let manifest = serde_json::json!({
        "k": k,
        "l": l,
        "count": count as u64,
        "min_s": min_s,
        "eps": eps,
        "threshold": thr,
        "pass": pass,
        "sha256_table_hex": hex,
        "sha256_exec_hex": "test",
        "generator_cmdline": "test",
        "pkg_version": "test",
        "build_git_rev": "test",
        "build_rustc": "test",
        "os_arch": "test",
        "gen_ts": "test",
        "file_ver": 1,
    });
    let manifest_path = dir_path.join("cert_v1_k4_l8.json");
    let mut mf = File::create(&manifest_path)?;
    mf.write_all(serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    // Run verify against v1 synthetic files
    let mut cmd = Command::cargo_bin("collatz_cert")?;
    cmd.current_dir(dir_path)
        .args([
            "verify", "--k", "4", "--l", "8",
            "--table", "table_v1_k4_l8.bin",
            "--manifest", "cert_v1_k4_l8.json",
            "--threads", "2",
        ])
        .assert()
        .success();

    Ok(())
}
