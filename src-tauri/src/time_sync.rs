//! 文件说明：通过多个国内 NTP 服务测量本机时钟偏差并选择最快可用时钟源。

use crate::app_log;
use serde::Serialize;
use std::{
    net::{ToSocketAddrs, UdpSocket},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

/// 内置的国内 NTP 服务地址，不对用户开放编辑。
const NTP_SERVERS: [&str; 6] = [
    "ntp.ntsc.ac.cn:123",
    "ntp.aliyun.com:123",
    "ntp1.aliyun.com:123",
    "time1.cloud.tencent.com:123",
    "time1.tencentyun.com:123",
    "cn.pool.ntp.org:123",
];

const NTP_UNIX_OFFSET_SECONDS: i64 = 2_208_988_800;
const NTP_PACKET_SIZE: usize = 48;
const NTP_TIMEOUT: Duration = Duration::from_millis(900);

/// 描述一次成功的北京时间校准结果。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TimeSyncSnapshot {
    pub(crate) server: String,
    pub(crate) offset_ms: i64,
    pub(crate) round_trip_ms: u64,
    pub(crate) synced_at_unix_ms: i64,
}

/// 并发探测内置 NTP 服务并返回往返时间最短的有效结果。
#[tauri::command]
pub(crate) async fn sync_beijing_time() -> Result<TimeSyncSnapshot, String> {
    let probes = NTP_SERVERS
        .iter()
        .map(|server| {
            let server = *server;
            thread::spawn(move || probe_server(server))
        })
        .collect::<Vec<_>>();

    let mut results = probes
        .into_iter()
        .filter_map(|probe| probe.join().ok().flatten())
        .collect::<Vec<_>>();
    results.sort_by_key(|result| result.round_trip_ms);

    let result = results
        .into_iter()
        .next()
        .ok_or_else(|| "内置 NTP 服务器均未返回有效时间".to_string())?;

    app_log::info(format!(
        "北京时间校准成功：服务器={}，偏差={}ms，往返={}ms",
        result.server, result.offset_ms, result.round_trip_ms
    ));
    Ok(result)
}

/// 向单个 NTP 服务发送请求并计算本机相对标准时间的偏差。
fn probe_server(server: &str) -> Option<TimeSyncSnapshot> {
    let addresses = server.to_socket_addrs().ok()?;
    for address in addresses {
        let socket = UdpSocket::bind("0.0.0.0:0").ok()?;
        socket.set_read_timeout(Some(NTP_TIMEOUT)).ok()?;
        socket.set_write_timeout(Some(NTP_TIMEOUT)).ok()?;

        let mut request = [0_u8; NTP_PACKET_SIZE];
        request[0] = 0x1b;
        let sent_at = SystemTime::now();
        let started = Instant::now();
        socket.send_to(&request, address).ok()?;

        let mut response = [0_u8; NTP_PACKET_SIZE];
        let (received, _) = socket.recv_from(&mut response).ok()?;
        if received < NTP_PACKET_SIZE || response[0] & 0x07 != 4 {
            continue;
        }

        let stratum = response[1];
        if stratum == 0 || stratum > 15 {
            continue;
        }

        let server_received_at = ntp_timestamp_to_unix_ms(&response[32..40])?;
        let server_transmitted_at = ntp_timestamp_to_unix_ms(&response[40..48])?;
        let received_at = SystemTime::now();
        let sent_at_ms = system_time_to_unix_ms(sent_at)?;
        let received_at_ms = system_time_to_unix_ms(received_at)?;
        let offset_ms =
            ((server_received_at - sent_at_ms) + (server_transmitted_at - received_at_ms)) / 2;

        return Some(TimeSyncSnapshot {
            server: server.trim_end_matches(":123").to_string(),
            offset_ms,
            round_trip_ms: started.elapsed().as_millis() as u64,
            synced_at_unix_ms: received_at_ms,
        });
    }

    None
}

/// 将 NTP 64 位时间戳转换为 Unix 毫秒时间戳。
fn ntp_timestamp_to_unix_ms(bytes: &[u8]) -> Option<i64> {
    if bytes.len() != 8 {
        return None;
    }

    let seconds = u32::from_be_bytes(bytes[0..4].try_into().ok()?) as i64;
    let fraction = u32::from_be_bytes(bytes[4..8].try_into().ok()?) as i64;
    if seconds < NTP_UNIX_OFFSET_SECONDS {
        return None;
    }

    let unix_seconds = seconds - NTP_UNIX_OFFSET_SECONDS;
    Some(unix_seconds * 1_000 + fraction * 1_000 / (1_i64 << 32))
}

/// 将系统时间转换为 Unix 毫秒时间戳。
fn system_time_to_unix_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ntp_timestamp_converts_unix_epoch() {
        let bytes = (NTP_UNIX_OFFSET_SECONDS as u32)
            .to_be_bytes()
            .into_iter()
            .chain([0_u8; 4])
            .collect::<Vec<_>>();

        assert_eq!(ntp_timestamp_to_unix_ms(&bytes), Some(0));
    }

    #[test]
    fn invalid_ntp_timestamp_is_rejected() {
        assert_eq!(ntp_timestamp_to_unix_ms(&[0_u8; 8]), None);
        assert_eq!(ntp_timestamp_to_unix_ms(&[0_u8; 7]), None);
    }
}
