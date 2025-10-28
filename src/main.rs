use clap::{Parser, Subcommand};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write, BufReader};
use std::path::{Path, PathBuf};
use std::cmp::{min, max};

/// CLI
#[derive(Parser)]
#[command(author, version, about="Collatz drift certificate")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    Gen {
        #[arg(long, default_value_t = 24)] k: u32,
        #[arg(long, default_value_t = 256)] l: u32,
        #[arg(long, default_value_t = 0)] threads: usize,
        /// Optional output table path; defaults to table_k{K}_l{L}_v2.bin
        #[arg(long)] out_table: Option<PathBuf>,
        /// Optional output manifest path; defaults to cert_k{K}_l{L}_v2.json
        #[arg(long)] out_manifest: Option<PathBuf>,
    },
    Verify {
        #[arg(long)] k: u32,
        #[arg(long)] l: u32,
        #[arg(long)] table: PathBuf,
        #[arg(long)] manifest: PathBuf,
        #[arg(long, default_value_t = 0)] threads: usize,
    },
    /// Compute summary stats and histogram for a table file
    Stats {
        /// Path to table file (v1 or v2)
        #[arg(long)] table: PathBuf,
        /// Number of bins in histogram
        #[arg(long, default_value_t = 50)] bins: usize,
        /// Output CSV for histogram (bin_lo,bin_hi,count)
        #[arg(long)] out_csv: Option<PathBuf>,
    },
    /// Pack table+manifest into tar.gz and emit sha256; optionally write CHECKSUMS.sha256
    Pack {
        #[arg(long)] table: PathBuf,
        #[arg(long)] manifest: PathBuf,
        #[arg(long)] out: Option<PathBuf>,
        /// Also write CHECKSUMS.sha256 next to archive
        #[arg(long, default_value_t = false)] checksums: bool,
    },
}

#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    magic: [u8; 4],
    ver: u32,
    k: u32,
    l: u32,
    count: u64,
    _reserved: [u8; 8],
}

#[derive(Serialize, Deserialize)]
struct Manifest {
    k: u32,
    l: u32,
    count: u64,
    min_s: u32,
    eps: f64,
    threshold: u32,
    pass: bool,
    sha256_table_hex: String,
    sha256_exec_hex: String,
    generator_cmdline: String,
    pkg_version: String,
    build_git_rev: String,
    build_rustc: String,
    os_arch: String,
    gen_ts: String,
    #[serde(default)]
    file_ver: u32,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Gen { k, l, threads, out_table, out_manifest } =>
            gen(k, l, threads, out_table, out_manifest),
        Cmd::Verify { k, l, table, manifest, threads } =>
            verify(k, l, table, manifest, threads),
        Cmd::Stats { table, bins, out_csv } => stats(table, bins, out_csv),
        Cmd::Pack { table, manifest, out, checksums } => pack(table, manifest, out, checksums),
    }
}

fn gen(k: u32, l: u32, threads: usize, out_table: Option<PathBuf>, out_manifest: Option<PathBuf>) -> anyhow::Result<()> {
    anyhow::ensure!((2..=28).contains(&k), "k in [2,28]");
    anyhow::ensure!(l >= 1, "l >= 1");

    let nthreads = if threads == 0 {
        std::thread::available_parallelism()?.get()
    } else { threads };
    eprintln!("threads={}", nthreads);

    let count: u64 = 1u64 << (k - 1);
    let mask: u64 = (1u64 << k) - 1;

    let min_s_atomic = std::sync::atomic::AtomicU32::new(u32::MAX);
    let mut table: Vec<u32> = vec![0; count as usize];

    let pool = rayon::ThreadPoolBuilder::new().num_threads(nthreads).build()?;
    pool.install(|| {
        table.par_iter_mut().enumerate().for_each(|(idx, slot)| {
            let mut m = ((idx as u64) << 1) | 1;
            let mut s: u64 = 0;
            for _ in 0..l {
                let t = 3u64.wrapping_mul(m & mask).wrapping_add(1);
                let e = t.trailing_zeros() as u64;
                s += e;
                m = (t >> e) & mask;
            }
            let s32 = s.min(u32::MAX as u64) as u32;
            *slot = s32;
            loop {
                let cur = min_s_atomic.load(std::sync::atomic::Ordering::Relaxed);
                if s32 < cur {
                    if min_s_atomic.compare_exchange(
                        cur, s32,
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed
                    ).is_ok() { break; }
                } else { break; }
            }
        });
    });

    // v2 format uses u32 entries; no overflow clipping required

    // header (v2 format: u32 entries)
    let file_ver: u32 = 2;
    let header = Header {
        magic: *b"CALT",
        ver: file_ver,
        k,
        l,
        count,
        _reserved: [0u8; 8],
    };

    // stream write with hashing to reduce peak memory
    let out_table = out_table.unwrap_or_else(|| PathBuf::from(format!("table_k{}_l{}_v2.bin", k, l)));
    let mut f = std::io::BufWriter::new(File::create(&out_table)?);
    f.write_all(&header.magic)?;
    f.write_all(&header.ver.to_le_bytes())?;
    f.write_all(&header.k.to_le_bytes())?;
    f.write_all(&header.l.to_le_bytes())?;
    f.write_all(&header.count.to_le_bytes())?;
    f.write_all(&header._reserved)?;

    let mut hasher = Sha256::new();
    for &v in &table {
        let bytes = (v as u32).to_le_bytes();
        hasher.update(&bytes);
        f.write_all(&bytes)?;
    }
    let digest = hasher.finalize();
    f.write_all(&digest)?;
    f.flush()?;

    let min_s = min_s_atomic.load(std::sync::atomic::Ordering::Relaxed);
    let thr = threshold_strict(l);
    let pass = (min_s as u32) >= thr;
    let eps = (min_s as f64) / (l as f64) - log2_3();

    let exe = std::env::current_exe()?;
    let sha_exec = sha256_file(&exe).unwrap_or_else(|_| "unknown".into());
    let ts = chrono::Utc::now().to_rfc3339();

    let out_manifest = out_manifest.unwrap_or_else(|| PathBuf::from(format!("cert_k{}_l{}_v2.json", k, l)));
    let manifest = Manifest {
        k,
        l,
        count,
        min_s,
        eps,
        threshold: thr,
        pass,
        sha256_table_hex: hex(&digest),
        sha256_exec_hex: sha_exec,
        generator_cmdline: std::env::args().collect::<Vec<_>>().join(" "),
        pkg_version: env!("CARGO_PKG_VERSION").to_string(),
        build_git_rev: option_env!("BUILD_GIT_REV").unwrap_or("unknown").to_string(),
        build_rustc: option_env!("BUILD_RUSTC").unwrap_or("unknown").to_string(),
        os_arch: format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH),
        gen_ts: ts,
        file_ver: file_ver,
    };
    let mut mf = File::create(&out_manifest)?;
    serde_json::to_writer_pretty(&mut mf, &manifest)?;
    mf.flush()?;

    eprintln!("OK gen: min_S={min_s} thr={thr} pass={pass} eps={:.6}", eps);
    eprintln!("table.sha256={}", hex(&digest));
    Ok(())
}

fn verify(k: u32, l: u32, table_path: PathBuf, manifest_path: PathBuf, threads: usize) -> anyhow::Result<()> {
    let nthreads = if threads == 0 {
        std::thread::available_parallelism()?.get()
    } else { threads };
    eprintln!("threads={}", nthreads);

    let mut data = Vec::new();
    File::open(&table_path)?.read_to_end(&mut data)?;
    anyhow::ensure!(data.len() >= 64, "file too small");

    anyhow::ensure!(&data[0..4] == b"CALT", "bad magic");
    let ver = u32::from_le_bytes(data[4..8].try_into()?);
    anyhow::ensure!(ver == 1 || ver == 2, "bad version");
    let k_file = u32::from_le_bytes(data[8..12].try_into()?);
    let l_file = u32::from_le_bytes(data[12..16].try_into()?);
    let count_file = u64::from_le_bytes(data[16..24].try_into()?);
    anyhow::ensure!(k == k_file && l == l_file, "K/L mismatch");
    let count = count_file as usize;

    let width: usize = if ver == 1 { 2 } else { 4 };
    let need = 32 + count * width + 32;
    anyhow::ensure!(data.len() == need, "bad file length");

    let table_bytes = &data[32..(32 + count * width)];
    let trailer = &data[(32 + count * width)..(32 + count * width + 32)];
    let mut hasher = Sha256::new();
    hasher.update(table_bytes);
    let digest = hasher.finalize();
    anyhow::ensure!(trailer == digest.as_slice(), "table sha256 mismatch");

    // parse table
    let mut table: Vec<u32> = Vec::with_capacity(count);
    if ver == 1 {
        for i in 0..count {
            let lo = table_bytes[2 * i] as u16;
            let hi = (table_bytes[2 * i + 1] as u16) << 8;
            table.push((lo | hi) as u32);
        }
    } else {
        for i in 0..count {
            let off = 4 * i;
            let v = u32::from_le_bytes([
                table_bytes[off],
                table_bytes[off + 1],
                table_bytes[off + 2],
                table_bytes[off + 3],
            ]);
            table.push(v);
        }
    }

    let mask: u64 = (1u64 << k) - 1;
    let recomputed_min = std::sync::atomic::AtomicU32::new(u32::MAX);
    let ok = std::sync::atomic::AtomicBool::new(true);

    let pool = rayon::ThreadPoolBuilder::new().num_threads(nthreads).build()?;
    pool.install(|| {
        (0..count).into_par_iter().for_each(|idx| {
            let mut m = ((idx as u64) << 1) | 1;
            let mut s: u64 = 0;
            for _ in 0..l {
                let t = 3u64.wrapping_mul(m & mask).wrapping_add(1);
                let e = t.trailing_zeros() as u64;
                s += e;
                m = (t >> e) & mask;
            }
            let s32 = s.min(u32::MAX as u64) as u32;
            if s32 != table[idx] as u32 {
                ok.store(false, std::sync::atomic::Ordering::Relaxed);
            }
            loop {
                let cur = recomputed_min.load(std::sync::atomic::Ordering::Relaxed);
                if s32 < cur {
                    if recomputed_min.compare_exchange(
                        cur, s32,
                        std::sync::atomic::Ordering::Relaxed,
                        std::sync::atomic::Ordering::Relaxed
                    ).is_ok() { break; }
                } else { break; }
            }
        });
    });

    anyhow::ensure!(ok.load(std::sync::atomic::Ordering::Relaxed), "value mismatch");
    let min_s = recomputed_min.load(std::sync::atomic::Ordering::Relaxed);
    let thr = threshold_strict(l);
    let pass = (min_s as u32) >= thr;
    let eps = (min_s as f64) / (l as f64) - log2_3();

    // check manifest
    let mf: Manifest = serde_json::from_reader(File::open(&manifest_path)?)?;
    anyhow::ensure!(mf.k == k && mf.l == l && mf.count as usize == count, "manifest mismatch");
    anyhow::ensure!(mf.sha256_table_hex == hex(digest.as_slice()), "manifest sha256 mismatch");
    if mf.file_ver != 0 { anyhow::ensure!(mf.file_ver == ver, "manifest file_ver mismatch"); }
    // cross-check computed stats vs manifest
    anyhow::ensure!(
        mf.min_s == min_s,
        "manifest min_s mismatch: manifest={} computed={}", mf.min_s, min_s
    );
    let thr2 = threshold_strict(mf.l);
    anyhow::ensure!(
        mf.threshold == thr2,
        "manifest threshold mismatch: manifest={} expected={}", mf.threshold, thr2
    );
    anyhow::ensure!(
        mf.pass == pass,
        "manifest pass mismatch: manifest={} computed={}", mf.pass, pass
    );
    let eps2 = (min_s as f64) / (l as f64) - log2_3();
    anyhow::ensure!(
        (mf.eps - eps2).abs() < 1e-12,
        "manifest eps mismatch: manifest={} computed={}", mf.eps, eps2
    );

    eprintln!("verify: min_S={min_s} thr={thr} pass={pass} eps={:.6}", eps);
    Ok(())
}

fn read_table_bytes(path: &Path) -> anyhow::Result<(u32,u32,u64,u32,Vec<u32>)> {
    let mut data = Vec::new();
    File::open(path)?.read_to_end(&mut data)?;
    anyhow::ensure!(data.len() >= 64, "file too small");
    anyhow::ensure!(&data[0..4] == b"CALT", "bad magic");
    let ver = u32::from_le_bytes(data[4..8].try_into()?);
    anyhow::ensure!(ver == 1 || ver == 2, "bad version");
    let k_file = u32::from_le_bytes(data[8..12].try_into()?);
    let l_file = u32::from_le_bytes(data[12..16].try_into()?);
    let count_file = u64::from_le_bytes(data[16..24].try_into()?);
    let count = count_file as usize;
    let width: usize = if ver == 1 { 2 } else { 4 };
    let need = 32 + count * width + 32;
    anyhow::ensure!(data.len() == need, "bad file length");
    let table_bytes = &data[32..(32 + count * width)];
    let trailer = &data[(32 + count * width)..(32 + count * width + 32)];
    let mut hasher = Sha256::new();
    hasher.update(table_bytes);
    let digest = hasher.finalize();
    anyhow::ensure!(trailer == digest.as_slice(), "table sha256 mismatch");
    let mut table: Vec<u32> = Vec::with_capacity(count);
    if ver == 1 {
        for i in 0..count {
            let lo = table_bytes[2 * i] as u16;
            let hi = (table_bytes[2 * i + 1] as u16) << 8;
            table.push((lo | hi) as u32);
        }
    } else {
        for i in 0..count {
            let off = 4 * i;
            let v = u32::from_le_bytes([
                table_bytes[off], table_bytes[off+1], table_bytes[off+2], table_bytes[off+3]
            ]);
            table.push(v);
        }
    }
    Ok((k_file, l_file, count_file, ver, table))
}

fn stats(table_path: PathBuf, bins: usize, out_csv: Option<PathBuf>) -> anyhow::Result<()> {
    let (k, l, count_u64, ver, table) = read_table_bytes(&table_path)?;
    let count = count_u64 as usize;
    anyhow::ensure!(count > 0, "empty table");
    let mut mn = u32::MAX; let mut mx = 0u32; let mut sum: f64 = 0.0;
    for &v in &table { mn = min(mn, v); mx = max(mx, v); sum += v as f64; }
    let mean = sum / (count as f64);
    let thr = threshold_strict(l);
    let eps = (mn as f64) / (l as f64) - log2_3();
    // histogram
    let bins = bins.max(1);
    let lo = mn as i64; let hi = mx.max(mn+1) as i64; // avoid zero width
    let width = (hi - lo) as f64 / (bins as f64);
    let mut hist = vec![0usize; bins];
    for &v in &table {
        let idx = (((v as i64 - lo) as f64) / width).floor() as isize;
        let idx = idx.clamp(0, (bins as isize)-1) as usize;
        hist[idx] += 1;
    }
    eprintln!("stats: K={k} L={l} ver={ver} count={count}");
    eprintln!("  min_S={mn} max_S={mx} mean={:.3}", mean);
    eprintln!("  thr={thr} pass(min)={}" , (mn as u32) >= thr);
    eprintln!("  eps(min)={:.6}", eps);
    if let Some(csv) = out_csv {
        let mut w = std::io::BufWriter::new(File::create(csv)?);
        writeln!(w, "bin_lo,bin_hi,count")?;
        for i in 0..bins {
            let b_lo = lo as f64 + (i as f64)*width;
            let b_hi = lo as f64 + ((i+1) as f64)*width;
            writeln!(w, "{:.6},{:.6},{}", b_lo, b_hi, hist[i])?;
        }
    }
    Ok(())
}

fn pack(table_path: PathBuf, manifest_path: PathBuf, out: Option<PathBuf>, checksums: bool) -> anyhow::Result<()> {
    // verify and extract header fields
    let (k, l, _count, ver, _table) = read_table_bytes(&table_path)?;
    // default out name
    let out_path = out.unwrap_or_else(|| PathBuf::from(format!("cert_k{}_l{}_v{}.tar.gz", k, l, ver)));
    let tar_gz = File::create(&out_path)?;
    let enc = flate2::write::GzEncoder::new(tar_gz, flate2::Compression::default());
    let mut tarb = tar::Builder::new(enc);
    // add files with just their basenames
    let table_name = table_path.file_name().unwrap_or_else(|| std::ffi::OsStr::new("table.bin"));
    let manifest_name = manifest_path.file_name().unwrap_or_else(|| std::ffi::OsStr::new("manifest.json"));
    tarb.append_path_with_name(&table_path, table_name)?;
    tarb.append_path_with_name(&manifest_path, manifest_name)?;
    let enc = tarb.into_inner()?; // GzEncoder
    let mut inner = enc.finish()?; // File
    inner.flush()?;
    // compute sha256 of archive
    let sha = sha256_file(&out_path)?;
    println!("tar.gz sha256={} file={}", sha, out_path.display());
    if checksums {
        let mut f = File::create("CHECKSUMS.sha256")?;
        writeln!(f, "{}  {}", sha, out_path.file_name().unwrap().to_string_lossy())?;
    }
    Ok(())
}

#[inline]
fn log2_3() -> f64 { 3f64.log2() }

#[inline]
fn threshold_strict(l: u32) -> u32 {
    ((l as f64)*log2_3()).floor() as u32 + 1
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

fn sha256_file(p: &Path) -> anyhow::Result<String> {
    let f = File::open(p)?;
    let mut r = BufReader::new(f);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 8192];
    loop {
        let n = r.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}
