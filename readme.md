# Collatz Drift Certificate

[![CI](https://img.shields.io/github/actions/workflow/status/andysay1/Collatz_Drift_Certificate/ci.yml?branch=main)](https://github.com/andysay1/Collatz_Drift_Certificate/actions/workflows/ci.yml)
[![Bench](https://img.shields.io/github/actions/workflow/status/andysay1/Collatz_Drift_Certificate/bench.yml?label=bench&branch=main)](https://github.com/andysay1/Collatz_Drift_Certificate/actions/workflows/bench.yml)

## Описание

`collatz_cert` — генератор и верификатор **сертификатов отрицательного дрейфа** для ускоренных траекторий гипотезы Коллатца.

Идея:

-   Для всех нечётных остатков `r mod 2^K` считается сумма степеней двойки `S_r` на блоке длины `L`.
-   Если `min(S_r / L) > log2(3)`, то все траектории обладают отрицательным дрейфом и нет нетривиальных циклов.

Таким образом, гипотеза Коллатца сводится к проверяемому компьютерному сертификату.

---

## Структура сертификата

-   v2 (по умолчанию): `table_k24_l256_v2.bin` — бинарный файл с таблицей всех `S_r` (u32, ver=2).
-   v1 (совместимость чтения): `table_k24_l256.bin` — старая версия (u16, ver=1).
-   `cert_k24_l256_v2.json` — манифест (K,L, min_S, eps, sha256, метаданные сборки, file_ver).
-   `CHECKSUMS.sha256` — контрольные суммы.
-   Архив: `cert_k24_l256.tar.gz`.

SHA-256 архива: cert_k24_l256.tar.gz

9ea7c4fae48d04b40c394b2924a7aea5c2cd5a870e323c4859f3988591a16444

## Установка

```bash
git clone
cd collatz_cert

cargo build --release


Генерация сертификата (v2 по умолчанию)

target/release/collatz_cert gen \
  --k 24 --l 256 --threads 24

# Выходные файлы по умолчанию:
#  table_k24_l256_v2.bin, cert_k24_l256_v2.json

Верификация сертификата

target/release/collatz_cert verify \
  --k 24 --l 256 \
  --table table_k24_l256_v2.bin \
  --manifest cert_k24_l256_v2.json \
  --threads 24

# Также поддерживается проверка старого формата v1
#  --table table_k24_l256.bin --manifest cert_k24_l256.json

## Вау‑фактор: статистика, упаковка, бенчмарки

- Статистика и гистограммы (CSV):

```bash
# Быстрый обзор таблицы и eps
./target/release/collatz_cert stats --table table_k24_l256_v2.bin --bins 100 --out-csv hist_k24_l256.csv

# Вывод:
# stats: K=24 L=256 ver=2 count=8388608
#   min_S=442 max_S=... mean=...
#   thr=406 pass(min)=true
#   eps(min)=0.141600
```

- Упаковка артефактов (tar.gz + sha256):

```bash
./target/release/collatz_cert pack \
  --table table_k24_l256_v2.bin \
  --manifest cert_k24_l256_v2.json \
  --out cert_k24_l256_v2.tar.gz \
  --checksums

# Выводит строку с sha256 и пишет CHECKSUMS.sha256
```

- Бенчмарки (примерная производительность на малых параметрах):

```bash
cargo bench --bench compute

# Откроет HTML‑отчёт Criterion в target/criterion/report

## CI и релизы

- GitHub Actions
  - CI: `.github/workflows/ci.yml` — сборка, тесты и смоук `gen/verify`.
  - Bench: `.github/workflows/bench.yml` — ручной запуск бенчмарков с выгрузкой отчётов.
- Бейджи: настроены на `andysay1/Collatz_Drift_Certificate` и ветку `main`.
- Паблиш скрипт: `scripts/publish_cert.sh`

```bash
# Пример публикации артефактов (архив, checksums, summary, histogram CSV) в dist/
scripts/publish_cert.sh -k 24 -l 256 -t 24 -o dist
```
```

## Тестирование

```bash
# Запуск всего набора тестов
cargo test

# Что проверяют тесты
# - v2 roundtrip: генерация по умолчанию (u32, ver=2) и последующая верификация
# - v1 совместимость: синтетический небольшой файл ver=1 валидируется корректно
```

Проверка контрольных сумм

  shasum -a256 -c CHECKSUMS.sha256


```
