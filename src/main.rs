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
//! listup_jorei -o output
//! ```
//!
//! で起動します。
//!
//! オプションの各意味は以下のとおりです。
//!
//! - `-o`：解析で生成した情報を出力するフォルダ
//!
//! ---
//!
//! [MIT License](https://github.com/japanese-law-analysis/listup_jorei/blob/master/LICENSE)
//! (c) 2024 Naoki Kaneko (a.k.a. "puripuri2100")
//!

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;
use tracing::*;

#[derive(Debug, Clone, Parser)]
struct AppArgs {
  /// 検索する範囲の始端を<year>-<month>-<date>形式で与える
  #[clap(short, long)]
  start: Option<String>,
  /// 検索する範囲の終端を<year>-<month>-<date>形式で与える
  #[clap(short, long)]
  end: Option<String>,
  /// 出力先のフォルダ
  #[clap(short, long)]
  output: String,
  /// 一回のAPIアクセスで取得する値で、大きければ大きいほど相手のサーバに負担がかかる
  #[clap(default_value = "50")]
  rows: usize,
  /// 一回のrowについてのAPIアクセスが行われるたびにsleepする時間（ミリ秒）
  #[clap(default_value = "500")]
  sleep_time: u64,
}

fn gen_list_url(
  start: Option<(usize, usize, usize)>,
  end: Option<(usize, usize, usize)>,
  n: usize,
  rows: usize,
) -> String {
  let start_s = if let Some((y, m, d)) = start {
    format!("{y}-{m}-{d}")
  } else {
    String::from("1883-01-01")
  };
  let end_s = if let Some((y, m, d)) = end {
    format!("{y:0>4}-{m:0>2}-{d:0>2}")
  } else {
    String::from("NOW")
  };
  let start_n = rows * n;
  format!(
    r"https://jorei.slis.doshisha.ac.jp/api/reiki/select?f.municipality_id.facet.limit=1788&facet.mincount=1&facet.range=announcement_date&facet.range.gap=%2B1YEAR&facet.range.start={start_s}T00%3A00%3A00Z&facet.range.end={end_s}&hl=true&hl.fl=content&hl.usePhraseHighlighter=true&q=collection%3Alatest&start={start_n}&rows={rows}&fq=&facet=true&facet.field=municipality_type&facet.field=city&facet.field=type&facet.field=h_type&facet.field=municipality_id"
  )
}

fn gen_jorei_url(id: &str) -> String {
  format!(r"https://jorei.slis.doshisha.ac.jp/api/reiki/select?q=ids%3A{id}&all=true")
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiInfoListResponseDocs {
  collection: Vec<String>,
  collected_date: Vec<String>,
  updated_date: Vec<String>,
  municipality_id: String,
  prefecture: String,
  city: String,
  prefecture_kana: String,
  city_kana: String,
  municipality_type: String,
  area: String,
  id: String,
  reiki_id: String,
  h1: Option<String>,
  title: String,
  announcement_date: Option<String>,
  r#type: String,
  last_updated_date: Option<String>,
  reiki_dates: Option<Vec<String>>,
  reiki_numbers: Option<Vec<String>>,
  original_url: Option<String>,
  has_version: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiInfoListResponse {
  #[serde(rename = "numFound")]
  num_found: usize,
  start: usize,
  docs: Vec<JoreiInfoListResponseDocs>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiListResponse {
  response: JoreiInfoListResponse,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiInfoDocs {
  collection: Vec<String>,
  collected_date: Vec<String>,
  updated_date: Vec<String>,
  municipality_id: String,
  prefecture: String,
  city: String,
  prefecture_kana: String,
  city_kana: String,
  municipality_type: String,
  area: String,
  id: String,
  reiki_id: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  h1: Option<String>,
  title: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  announcement_date: Option<String>,
  r#type: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  last_updated_date: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  reiki_dates: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  reiki_numbers: Option<Vec<String>>,
  #[serde(skip_serializing_if = "Option::is_none")]
  original_url: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  reiki_url: Option<String>,
  has_version: bool,
  file_type: String,
  h_type: Vec<String>,
  content: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  collected_date_s: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  announcement_date_s: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  last_updated_date_s: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  updated_date_s: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiInfo {
  #[serde(rename = "numFound")]
  num_found: usize,
  start: usize,
  docs: Vec<JoreiInfoDocs>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct JoreiInfoResponse {
  response: JoreiInfo,
}

async fn write_docs(output: &str, id: &str, docs: &JoreiInfoDocs) -> Result<()> {
  let mut buf = File::create(format!("{output}/{id}.json")).await?;
  let s = serde_json::to_string_pretty(&docs)?;
  buf.write_all(s.as_bytes()).await?;
  Ok(())
}

async fn init_logger() -> Result<()> {
  let subscriber = tracing_subscriber::fmt()
    .with_max_level(tracing::Level::INFO)
    .finish();
  tracing::subscriber::set_global_default(subscriber)?;
  Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
  let args = AppArgs::parse();

  init_logger().await?;

  let re_day = Regex::new(r"(?<y>\d{4})-(?<m>\d{2})-(?<d>\d{2})").unwrap();
  let start = if let Some(s) = &args.start {
    let caps = re_day.captures(s).with_context(|| anyhow!("err"))?;
    let y = &caps["y"].parse::<usize>()?;
    let m = &caps["m"].parse::<usize>()?;
    let d = &caps["d"].parse::<usize>()?;
    Some((*y, *m, *d))
  } else {
    None
  };
  let end = if let Some(s) = &args.end {
    let caps = re_day.captures(s).with_context(|| anyhow!("err"))?;
    let y = &caps["y"].parse::<usize>()?;
    let m = &caps["m"].parse::<usize>()?;
    let d = &caps["d"].parse::<usize>()?;
    Some((*y, *m, *d))
  } else {
    None
  };

  // jorei.slis.doshisa.ac.jpの証明書が壊れているので検証しない設定にする
  let client = reqwest::Client::builder()
    .danger_accept_invalid_certs(true)
    .build()?;

  let first_api_url = gen_list_url(start, end, 0, args.rows);

  let first_resp: JoreiListResponse = client.get(&first_api_url).send().await?.json().await?;
  let first_resp = first_resp.response;

  let all_size = first_resp.num_found;

  info!("number of all jorei: {all_size}");

  let mut stream = tokio_stream::iter(0..=(all_size / args.rows));
  while let Some(n) = stream.next().await {
    let list_api_url = gen_list_url(start, end, n, args.rows);

    let list_resp: JoreiListResponse = client.get(&list_api_url).send().await?.json().await?;
    let id_lst = list_resp.response.docs.iter().map(|d| &d.id);
    let mut id_stream = tokio_stream::iter(id_lst);
    while let Some(id) = id_stream.next().await {
      let api_url = gen_jorei_url(id);
      let jorei_info: JoreiInfoResponse = client.get(&api_url).send().await?.json().await?;
      let docs = &jorei_info.response.docs[0];
      write_docs(&args.output, id, docs).await?;
      info!("done: {}({})", docs.title, docs.id);
    }
    // 負荷を抑えるために500ミリ秒待つ
    info!("sleep");
    tokio::time::sleep(tokio::time::Duration::from_millis(args.sleep_time)).await;
  }
  info!("all done");
  Ok(())
}
