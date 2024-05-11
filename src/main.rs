//! 日本全国の条例の情報を<https://jorei.slis.doshisha.ac.jp/>をもとにローカルに保存するスクリプトです。
//!
//!
//! # CLIソフトウェアを使う
//!
//! ## インストール
//!
//! ```sh
//! cargo install --git "https://github.com/japanese-law-analysis/listup_jorei.git"
//! ```
//!
//! ## 使い方
//!
//! ```sh
//! listup_jorei --output output --index index --start 2022-01-01 --end 2022-12-31 --rows 50 --sleep-time 500
//! ```
//!
//! で起動します。
//!
//! オプションの各意味は以下のとおりです。
//!
//! - `--output`：解析で生成した情報を出力するフォルダ
//! - `--index`：条例の情報の一覧を出力するファイル
//! - `--start`：年範囲の始端（オプション）
//! - `--end`：年範囲の終端（オプション）
//! - `--rows`：一度の処理の重さ（オプション）
//! - `--sleep-time`：一度の処理ごとに挟まるスリープ時間（オプション）
//!
//! ---
//!
//! [MIT License](https://github.com/japanese-law-analysis/listup_jorei/blob/master/LICENSE)
//! (c) 2024 Naoki Kaneko (a.k.a. "puripuri2100")
//!

use anyhow::Result;
use chrono::{DateTime, Datelike, FixedOffset, TimeZone, Utc};
use clap::Parser;
use jplaw_data_types::{
  law::Date,
  listup::{JoreiData, JoreiInfo},
};
use jplaw_io::{flush_file_value_lst, gen_file_value_lst, init_logger, write_value_lst};
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use tracing::*;

#[derive(Debug, Clone, Parser)]
struct AppArgs {
  /// 検索する年の範囲の始端を与える
  #[clap(short, long)]
  start: Option<usize>,
  /// 検索する年の範囲の終端を与える
  #[clap(short, long)]
  end: Option<usize>,
  /// 出力する一覧ファイル名
  #[clap(short, long)]
  index: String,
  /// JSONデータの出力先フォルダ
  #[clap(short, long)]
  output: String,
  /// 一回のAPIアクセスで取得する値で、大きければ大きいほど相手のサーバに負担がかかる
  #[clap(short, long, default_value = "50")]
  rows: usize,
  /// 一回のrowについてのAPIアクセスが行われるたびにsleepする時間（ミリ秒）
  #[clap(short, long, default_value = "500")]
  sleep_time: u64,
}

fn gen_list_url(start: Option<usize>, end: Option<usize>, n: usize, rows: usize) -> String {
  let start_s = if let Some(y) = start {
    format!("{y:0>4}")
  } else {
    String::from("*")
  };
  let end_s = if let Some(y) = end {
    format!("{y:0>4}")
  } else {
    String::from("*")
  };
  let start_n = rows * n;
  format!(
    r"https://jorei.slis.doshisha.ac.jp/api/reiki/select?f.municipality_id.facet.limit=1788&facet.mincount=1&facet.range=announcement_date&facet.range.gap=%2B1YEAR&facet.range.start=1883-01-01T00%3A00%3A00Z&facet.range.end=NOW&q=collection%3Alatest%20AND%20announcement_date%3A%5B{start_s}%20TO%20{end_s}%5D&start={start_n}&rows={rows}&fq=&facet=true&facet.field=municipality_type&facet.field=city&facet.field=type&facet.field=h_type&facet.field=municipality_id"
  )
}

fn gen_jorei_url(id: &str) -> String {
  format!(r"https://jorei.slis.doshisha.ac.jp/api/reiki/select?q=ids%3A{id}&all=true")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiInfoResponseDocs {
  #[serde(default)]
  collection: Vec<String>,
  #[serde(default)]
  collected_date: Vec<String>,
  #[serde(default)]
  updated_date: Vec<DateTime<Utc>>,
  municipality_id: String,
  prefecture: Option<String>,
  city: Option<String>,
  prefecture_kana: Option<String>,
  city_kana: Option<String>,
  municipality_type: String,
  area: String,
  id: String,
  reiki_id: String,
  h1: Option<String>,
  title: String,
  announcement_date: Option<DateTime<Utc>>,
  r#type: String,
  last_updated_date: Option<DateTime<Utc>>,
  reiki_dates: Option<Vec<String>>,
  reiki_numbers: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  update_count: Option<usize>,
  original_url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  reiki_url: Option<String>,
  has_version: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  file_type: Option<String>,
  #[serde(skip_serializing_if = "Vec::is_empty", default)]
  h_type: Vec<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  content: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub collected_date_s: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub announcement_date_s: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub last_updated_date_s: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub updated_date_s: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiInfoResponse {
  #[serde(rename = "numFound")]
  num_found: usize,
  start: usize,
  docs: Vec<JoreiInfoResponseDocs>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiApiResponse {
  response: JoreiInfoResponse,
}

fn utc_to_date(datatime: &DateTime<Utc>) -> Date {
  let offset = FixedOffset::east_opt(9 * 3600).unwrap();
  let local = offset.from_utc_datetime(&datatime.naive_utc());
  Date::gen_from_ad(
    local.year() as usize,
    local.month() as usize,
    local.day() as usize,
  )
}

async fn gen_jorei_data(docs: &JoreiInfoResponseDocs) -> JoreiData {
  JoreiData {
    collection: docs.collection.clone(),
    collected_date: docs.collected_date.clone(),
    updated_date: docs.updated_date.iter().map(utc_to_date).collect(),
    municipality_id: docs.municipality_id.clone(),
    prefecture: docs.prefecture.clone(),
    city: docs.city.clone(),
    prefecture_kana: docs.prefecture_kana.clone(),
    city_kana: docs.city_kana.clone(),
    municipality_type: docs.municipality_type.clone(),
    area: docs.area.clone(),
    id: docs.id.clone(),
    reiki_id: docs.reiki_id.clone(),
    h1: docs.h1.clone(),
    title: docs.title.clone(),
    announcement_date: docs.announcement_date.map(|t| utc_to_date(&t)),
    jorei_type: docs.r#type.clone(),
    last_updated_date: docs.last_updated_date.map(|t| utc_to_date(&t)),
    reiki_dates: docs.reiki_dates.clone(),
    reiki_numbers: docs.reiki_numbers.clone(),
    original_url: docs.original_url.clone(),
    reiki_url: docs.reiki_url.clone(),
    has_version: docs.has_version,
    file_type: docs.file_type.clone().unwrap(),
    h_type: docs.h_type.clone(),
    content: docs.content.clone(),
    collected_date_s: docs.collected_date_s.clone(),
    announcement_date_s: docs.announcement_date_s.clone(),
    last_updated_date_s: docs.last_updated_date_s.clone(),
    updated_date_s: docs.updated_date_s.clone(),
  }
}

async fn gen_jorei_info(docs: &JoreiInfoResponseDocs) -> JoreiInfo {
  JoreiInfo {
    title: docs.title.clone(),
    reiki_id: docs.reiki_id.clone(),
    id: docs.id.clone(),
    prefecture: docs.prefecture.clone(),
    city: docs.city.clone(),
    announcement_date: docs.announcement_date.map(|t| utc_to_date(&t)),
    updated_date: docs.last_updated_date.map(|t| utc_to_date(&t)),
  }
}

async fn write_docs(output: &str, id: &str, data: &JoreiData) -> Result<()> {
  let mut buf = File::create(format!("{output}/{id}.json")).await?;
  let s = serde_json::to_string_pretty(&data)?;
  buf.write_all(s.as_bytes()).await?;
  buf.flush().await?;
  Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
  let args = AppArgs::parse();

  init_logger().await?;

  // jorei.slis.doshisa.ac.jpの証明書が壊れているので検証しない設定にする
  let client = reqwest::Client::builder()
    .danger_accept_invalid_certs(true)
    .build()?;

  let first_api_url = gen_list_url(args.start, args.end, 0, args.rows);

  let first_resp: JoreiApiResponse = client.get(&first_api_url).send().await?.json().await?;
  let first_resp = first_resp.response;

  let all_size = first_resp.num_found;

  info!("number of all jorei: {all_size}");

  let mut index_file = gen_file_value_lst(&args.index).await?;

  let mut stream = tokio_stream::iter(0..=(all_size / args.rows));
  while let Some(n) = stream.next().await {
    let list_api_url = gen_list_url(args.start, args.end, n, args.rows);

    let list_resp: JoreiApiResponse = client.get(&list_api_url).send().await?.json().await?;
    let id_lst = list_resp.response.docs.iter().map(|d| &d.id);
    let mut id_stream = tokio_stream::iter(id_lst);
    while let Some(id) = id_stream.next().await {
      let api_url = gen_jorei_url(id);
      let jorei_info: JoreiApiResponse = client.get(&api_url).send().await?.json().await?;
      let docs = &jorei_info.response.docs[0];
      let data = gen_jorei_data(docs).await;
      write_docs(&args.output, id, &data).await?;
      let info = gen_jorei_info(docs).await;
      write_value_lst(&mut index_file, info).await?;
      info!(
        "done: {}({}) at ({})",
        docs.title,
        docs.id,
        docs
          .clone()
          .announcement_date_s
          .unwrap_or("None".to_string())
      );
    }
    // 負荷を抑えるために500ミリ秒待つ
    info!("sleep");
    tokio::time::sleep(tokio::time::Duration::from_millis(args.sleep_time)).await;
  }
  flush_file_value_lst(&mut index_file).await?;
  info!("all done");
  Ok(())
}
