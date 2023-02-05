mod mp3;

use std::{
    collections::HashMap,
    env::args,
    io::{Cursor, Read, Write},
    process::{Command, Stdio},
};

use futures::StreamExt;
use regex::Regex;
use reqwest::{self, header, Client, ClientBuilder};
use rodio::{OutputStream, Sink};
use serde::{Deserialize, Serialize};

use crate::mp3::Mp3StreamDecoder;

#[derive(Debug, Deserialize, Serialize)]
struct InvocationInfo {
    #[serde(rename = "exec-duration-millis")]
    exec_duration_millis: i32,
    hostname: String,

    #[serde(rename = "req-id")]
    req_id: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct TrackInfoResult {
    #[serde(rename = "bitrateInKbps")]
    bitrate_in_kbps: i32,
    codec: String,
    direct: bool,

    #[serde(rename = "downloadInfoUrl")]
    download_info_url: String,
    gain: bool,
    preview: bool,
}

#[derive(Debug, Deserialize, Serialize)]
struct TrackInfo {
    #[serde(rename = "invocationInfo")]
    invocation_info: InvocationInfo,
    result: Vec<TrackInfoResult>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename = "download-info")]
struct DownloadInfo {
    host: String,
    path: String,
    ts: String,
    region: i32,
    s: String,
}

#[tokio::main]
async fn main() {
    assert!(args().len() > 1, "Pass a track id");

    let id = match args().nth(1).unwrap().starts_with("http") {
        true => {
            let argument = args().nth(1).unwrap();
            Regex::new(r"(\d{8})")
                .unwrap()
                .captures_iter(&argument)
                .nth(1)
                .and_then(|cap| Some(cap.get(0).unwrap()))
                .unwrap()
                .as_str()
                .into()
        }
        false => args().nth(1).unwrap(),
    };

    let token = std::env::var("YANDEX_MUSIC_TOKEN").expect("YANDEX_MUSIC_TOKEN must be set");

    let mut auth_value = header::HeaderValue::from_str(&token).unwrap();
    auth_value.set_sensitive(true);

    let mut headers = reqwest::header::HeaderMap::new();
    headers.insert("Authorization", auth_value);
    headers.insert("User-Agent", "Windows 10".parse().unwrap());

    let client = Client::builder()
        .default_headers(headers.to_owned())
        .build()
        .unwrap();

    println!("Trying to get track info for {}", &id);

    let result = client
        .get(std::format!("https://api.music.yandex.net/tracks/{id}/download-info?can_use_streaming=false"))
        .send()
        .await
        .unwrap();

    if result.status().is_success() {
        let download_info = result.json::<TrackInfo>().await.unwrap();
        let download_urls: Vec<&String> = download_info
            .result
            .iter()
            .filter(|x| x.codec == "mp3")
            .filter(|x| x.bitrate_in_kbps >= 320)
            .map(|x| &x.download_info_url)
            .collect();

        if let Some(download_url) = download_urls.first() {
            println!("Found {}", download_url);

            println!("Trying to get download info for {}", &id);
            let result = client
                .get(download_url.to_owned())
                .send()
                .await
                .unwrap()
                .text()
                .await
                .unwrap();

            let download_data: DownloadInfo =
                serde_xml_rs::from_str(std::str::from_utf8(result.as_bytes()).unwrap()).unwrap();

            println!("Found!");

            let seed = md5::compute(std::format!(
                "XGRlBW9FXlekgbPrRHuSiA{}{}",
                &download_data.path[1..download_data.path.len()],
                &download_data.s
            ));

            println!("secret {:x}", seed);

            let mp3_url = format!(
                "https://{}/get-mp3/{:x}/{}{}",
                download_data.host.to_owned(),
                seed,
                download_data.ts,
                download_data.path.to_owned()
            );
            println!("Track url {:?}", &mp3_url);

            std::thread::spawn(move || {
                let blocking_client = reqwest::blocking::Client::builder()
                    .default_headers(headers.to_owned())
                    .build()
                    .unwrap();

                let (_stream, stream_handle) = OutputStream::try_default().unwrap();

                let mut response = blocking_client.get(&mp3_url).send().unwrap();
                let source = Mp3StreamDecoder::new(response).unwrap();
                let sink = Sink::try_new(&stream_handle).unwrap();
                sink.append(source);
                sink.play();

                sink.sleep_until_end();
            })
            .join()
            .unwrap();

            // let mut response = client.get(&mp3_url).send().await.unwrap();
            // let mut stream = response.bytes_stream();

            // let mut child = Command::new("ffplay")
            // 	.arg("-").arg("-autoexit")/*.arg("-nodisp") */
            // 	.stdin(Stdio::piped()).spawn().unwrap();

            // let child_stdin = child.stdin.as_mut().unwrap();

            // while let Ok(item) = stream.next().await.unwrap() {
            // 	child_stdin.write(&item).unwrap();
            // }

            // drop(child_stdin);
            // let exit = child.wait_with_output().unwrap();
        }
    } else {
        println!("{:?}", result.text().await.unwrap());
    }
}
