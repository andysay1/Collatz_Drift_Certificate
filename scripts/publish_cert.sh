#!/usr/bin/env bash
set -euo pipefail

usage() {
  echo "Usage: $0 -k <K> -l <L> [-t <threads>] [-o <outdir>]" >&2
}

K=""; L=""; THREADS="0"; OUTDIR="dist"
while getopts ":k:l:t:o:h" opt; do
  case $opt in
    k) K="$OPTARG" ;;
    l) L="$OPTARG" ;;
    t) THREADS="$OPTARG" ;;
    o) OUTDIR="$OPTARG" ;;
    h) usage; exit 0 ;;
    :) echo "Option -$OPTARG requires an argument" >&2; usage; exit 1;;
    \?) echo "Unknown option -$OPTARG" >&2; usage; exit 1;;
  esac
done

if [[ -z "$K" || -z "$L" ]]; then
  usage; exit 1
fi

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

echo "==> Building release"
cargo build --release

mkdir -p "$OUTDIR"

echo "==> Generating certificate (v2)"
./target/release/collatz_cert gen --k "$K" --l "$L" --threads "$THREADS"

TABLE="table_k${K}_l${L}_v2.bin"
MANIFEST="cert_k${K}_l${L}_v2.json"

echo "==> Stats & histogram"
./target/release/collatz_cert stats --table "$TABLE" --bins 128 --out-csv "$OUTDIR/hist_k${K}_l${L}.csv"

echo "==> Pack archive + checksums"
./target/release/collatz_cert pack --table "$TABLE" --manifest "$MANIFEST" --out "$OUTDIR/cert_k${K}_l${L}_v2.tar.gz" --checksums

echo "==> Move artifacts"
mv -f "$TABLE" "$MANIFEST" CHECKSUMS.sha256 "$OUTDIR"/

echo "==> Summary"
{
  echo "Collatz Drift Certificate"
  echo "K=$K L=$L THREADS=$THREADS"
  ./target/release/collatz_cert verify --k "$K" --l "$L" --table "$OUTDIR/$TABLE" --manifest "$OUTDIR/$MANIFEST" --threads "$THREADS" || true
} > "$OUTDIR/summary_k${K}_l${L}.txt"

echo "Done. Artifacts in $OUTDIR";

