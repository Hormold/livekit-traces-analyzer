//! PCAP file parsing for SIP/RTP analysis.
//!
//! Extracts useful information from network captures:
//! - SIP signaling (call setup, errors)
//! - RTP statistics (jitter, packet loss, latency)
//! - Audio stream analysis (talk time, silence detection)

use std::collections::HashMap;
use std::fs::File;
use std::path::Path;

use anyhow::{Context, Result};
use pcap_file::pcap::PcapReader;
use serde::Serialize;

/// Parsed PCAP analysis results.
#[derive(Debug, Clone, Serialize)]
pub struct PcapAnalysis {
    /// Total packets in capture
    pub total_packets: usize,
    /// Capture duration in seconds
    pub duration_sec: f64,
    /// SIP messages found
    pub sip_messages: Vec<SipMessage>,
    /// RTP stream statistics
    pub rtp_streams: Vec<RtpStream>,
    /// Call setup time (INVITE to 200 OK) in ms
    pub call_setup_ms: Option<f64>,
    /// Time from 200 OK to first RTP packet
    pub media_setup_ms: Option<f64>,
    /// Summary of issues found
    pub issues: Vec<String>,
    /// Network quality score (0-100)
    pub quality_score: u8,
}

/// A SIP message extracted from PCAP.
#[derive(Debug, Clone, Serialize)]
pub struct SipMessage {
    /// Timestamp (seconds from start)
    pub timestamp_sec: f64,
    /// SIP method or response code
    pub method: String,
    /// From header (phone number)
    pub from: Option<String>,
    /// To header (phone number)
    pub to: Option<String>,
    /// Call-ID header
    pub call_id: Option<String>,
}

/// RTP stream statistics.
#[derive(Debug, Clone, Serialize)]
pub struct RtpStream {
    /// Stream identifier (src:port -> dst:port)
    pub flow: String,
    /// Direction: "outgoing" or "incoming"
    pub direction: String,
    /// Number of packets
    pub packet_count: usize,
    /// Duration in seconds
    pub duration_sec: f64,
    /// Packets per second
    pub packets_per_sec: f64,
    /// Average jitter in ms
    pub jitter_avg_ms: f64,
    /// Max jitter in ms
    pub jitter_max_ms: f64,
    /// Packet loss percentage
    pub loss_pct: f64,
    /// Lost packet count
    pub lost_packets: usize,
    /// Out of order packets
    pub out_of_order: usize,
    /// Average packet size
    pub avg_packet_size: usize,
    /// Estimated codec (based on packet size/timing)
    pub codec_guess: String,
}

/// Internal: UDP packet info for RTP analysis
#[derive(Debug, Clone)]
struct UdpPacket {
    timestamp: f64,
    src_ip: String,
    src_port: u16,
    dst_ip: String,
    dst_port: u16,
    payload_len: usize,
    payload: Vec<u8>,
}

/// Parse a PCAP file and extract SIP/RTP information.
pub fn parse_pcap(path: &Path) -> Result<PcapAnalysis> {
    let file = File::open(path)
        .with_context(|| format!("Failed to open PCAP file: {}", path.display()))?;

    let mut reader = PcapReader::new(file)
        .with_context(|| format!("Failed to parse PCAP file: {}", path.display()))?;

    let mut total_packets = 0;
    let mut first_ts: Option<f64> = None;
    let mut last_ts: Option<f64> = None;
    let mut sip_messages: Vec<SipMessage> = Vec::new();
    let mut udp_packets: Vec<UdpPacket> = Vec::new();
    let mut invite_ts: Option<f64> = None;
    let mut ok_200_ts: Option<f64> = None;

    // Read all packets
    while let Some(packet) = reader.next_packet() {
        let packet = packet.context("Failed to read packet")?;
        total_packets += 1;

        // Calculate timestamp
        let ts_sec = packet.timestamp.as_secs() as f64
            + packet.timestamp.subsec_nanos() as f64 / 1e9;

        if first_ts.is_none() {
            first_ts = Some(ts_sec);
        }
        last_ts = Some(ts_sec);

        let rel_ts = ts_sec - first_ts.unwrap_or(0.0);
        let data = &packet.data;

        // Parse the packet - handle Linux cooked capture (SLL)
        // SLL header is 16 bytes, then IP header
        if data.len() < 16 {
            continue;
        }

        // Check for Linux cooked capture (link type 113)
        // Or try standard Ethernet (14 bytes)
        let ip_offset = if data.len() > 16 && data[0] == 0x00 && (data[1] == 0x00 || data[1] == 0x04) {
            16 // Linux cooked capture
        } else if data.len() > 14 {
            14 // Ethernet
        } else {
            continue;
        };

        if data.len() < ip_offset + 20 {
            continue;
        }

        let ip_data = &data[ip_offset..];

        // Check IP version (should be 4)
        let ip_version = (ip_data[0] >> 4) & 0x0F;
        if ip_version != 4 {
            continue;
        }

        let ip_header_len = ((ip_data[0] & 0x0F) as usize) * 4;
        if ip_data.len() < ip_header_len {
            continue;
        }

        let protocol = ip_data[9];
        let src_ip = format!("{}.{}.{}.{}", ip_data[12], ip_data[13], ip_data[14], ip_data[15]);
        let dst_ip = format!("{}.{}.{}.{}", ip_data[16], ip_data[17], ip_data[18], ip_data[19]);

        // TCP (protocol 6) - likely SIP
        if protocol == 6 && ip_data.len() >= ip_header_len + 20 {
            let tcp_data = &ip_data[ip_header_len..];
            let tcp_header_len = (((tcp_data[12] >> 4) & 0x0F) as usize) * 4;

            if tcp_data.len() > tcp_header_len {
                let payload = &tcp_data[tcp_header_len..];
                if let Some(sip) = try_parse_sip(payload, rel_ts) {
                    if sip.method == "INVITE" && invite_ts.is_none() {
                        invite_ts = Some(rel_ts);
                    }
                    if sip.method.starts_with("200") && ok_200_ts.is_none() && invite_ts.is_some() {
                        ok_200_ts = Some(rel_ts);
                    }
                    sip_messages.push(sip);
                }
            }
        }

        // UDP (protocol 17) - likely RTP
        if protocol == 17 && ip_data.len() >= ip_header_len + 8 {
            let udp_data = &ip_data[ip_header_len..];
            let src_port = u16::from_be_bytes([udp_data[0], udp_data[1]]);
            let dst_port = u16::from_be_bytes([udp_data[2], udp_data[3]]);
            let udp_len = u16::from_be_bytes([udp_data[4], udp_data[5]]) as usize;

            if udp_data.len() >= 8 && udp_len > 8 {
                let payload = if udp_data.len() >= udp_len {
                    udp_data[8..udp_len].to_vec()
                } else {
                    udp_data[8..].to_vec()
                };

                udp_packets.push(UdpPacket {
                    timestamp: rel_ts,
                    src_ip,
                    src_port,
                    dst_ip,
                    dst_port,
                    payload_len: payload.len(),
                    payload,
                });
            }
        }
    }

    // Calculate duration
    let duration_sec = match (first_ts, last_ts) {
        (Some(f), Some(l)) => l - f,
        _ => 0.0,
    };

    // Calculate call setup time
    let call_setup_ms = match (invite_ts, ok_200_ts) {
        (Some(inv), Some(ok)) => Some((ok - inv) * 1000.0),
        _ => None,
    };

    // Analyze RTP streams from UDP packets
    let rtp_streams = analyze_rtp_streams(&udp_packets, ok_200_ts);

    // Calculate media setup time (200 OK to first RTP)
    let media_setup_ms = if let Some(ok_ts) = ok_200_ts {
        rtp_streams.iter()
            .filter_map(|s| {
                // Find first packet time for this stream
                udp_packets.iter()
                    .filter(|p| {
                        let flow = format!("{}:{} -> {}:{}", p.src_ip, p.src_port, p.dst_ip, p.dst_port);
                        s.flow == flow
                    })
                    .map(|p| p.timestamp)
                    .next()
            })
            .min_by(|a, b| a.partial_cmp(b).unwrap())
            .map(|first_rtp| (first_rtp - ok_ts) * 1000.0)
    } else {
        None
    };

    // Identify issues
    let mut issues: Vec<String> = Vec::new();

    if let Some(setup) = call_setup_ms {
        if setup > 5000.0 {
            issues.push(format!("Slow call setup: {:.0}ms (expected <3s)", setup));
        } else if setup > 3000.0 {
            issues.push(format!("Moderate call setup delay: {:.0}ms", setup));
        }
    }

    if let Some(media_setup) = media_setup_ms {
        if media_setup > 500.0 {
            issues.push(format!("Slow media setup: {:.0}ms after call answered", media_setup));
        }
    }

    for stream in &rtp_streams {
        if stream.jitter_avg_ms > 30.0 {
            issues.push(format!(
                "High average jitter on {}: {:.1}ms (target <30ms)",
                stream.direction, stream.jitter_avg_ms
            ));
        }
        if stream.jitter_max_ms > 100.0 {
            issues.push(format!(
                "Jitter spike on {}: {:.1}ms max",
                stream.direction, stream.jitter_max_ms
            ));
        }
        if stream.loss_pct > 1.0 {
            issues.push(format!(
                "Packet loss on {}: {:.1}% ({} packets)",
                stream.direction, stream.loss_pct, stream.lost_packets
            ));
        } else if stream.loss_pct > 0.1 {
            issues.push(format!(
                "Minor packet loss on {}: {:.2}%",
                stream.direction, stream.loss_pct
            ));
        }
        if stream.out_of_order > 0 {
            issues.push(format!(
                "Out-of-order packets on {}: {}",
                stream.direction, stream.out_of_order
            ));
        }
    }

    // Check for SIP errors
    for msg in &sip_messages {
        if msg.method.starts_with("4") || msg.method.starts_with("5") || msg.method.starts_with("6") {
            issues.push(format!("SIP error: {} at {:.2}s", msg.method, msg.timestamp_sec));
        }
    }

    // Calculate quality score (0-100)
    let quality_score = calculate_quality_score(&rtp_streams, &issues);

    Ok(PcapAnalysis {
        total_packets,
        duration_sec,
        sip_messages,
        rtp_streams,
        call_setup_ms,
        media_setup_ms,
        issues,
        quality_score,
    })
}

/// Analyze UDP packets to extract RTP stream statistics.
fn analyze_rtp_streams(packets: &[UdpPacket], call_answered: Option<f64>) -> Vec<RtpStream> {
    // Group packets by flow (src:port -> dst:port)
    let mut flows: HashMap<String, Vec<&UdpPacket>> = HashMap::new();

    for pkt in packets {
        // Only consider packets after call was answered (if we know when)
        if let Some(answered) = call_answered {
            if pkt.timestamp < answered {
                continue;
            }
        }

        let flow = format!("{}:{} -> {}:{}", pkt.src_ip, pkt.src_port, pkt.dst_ip, pkt.dst_port);
        flows.entry(flow).or_default().push(pkt);
    }

    let mut streams: Vec<RtpStream> = Vec::new();

    for (flow, pkts) in flows {
        if pkts.len() < 10 {
            continue; // Skip tiny streams
        }

        // Determine direction based on port numbers (higher port usually = client)
        let first = pkts[0];
        let direction = if first.src_port > first.dst_port {
            "outgoing"
        } else {
            "incoming"
        }.to_string();

        // Calculate timing statistics
        let duration_sec = pkts.last().map(|p| p.timestamp).unwrap_or(0.0)
            - pkts.first().map(|p| p.timestamp).unwrap_or(0.0);

        let packets_per_sec = if duration_sec > 0.0 {
            pkts.len() as f64 / duration_sec
        } else {
            0.0
        };

        // Calculate jitter (variation in inter-packet arrival time)
        let (jitter_avg_ms, jitter_max_ms) = calculate_jitter(&pkts);

        // Try to detect packet loss by analyzing RTP sequence numbers
        let (loss_pct, lost_packets, out_of_order) = analyze_rtp_sequence(&pkts);

        // Average packet size
        let avg_packet_size = pkts.iter().map(|p| p.payload_len).sum::<usize>() / pkts.len().max(1);

        // Guess codec based on packet size and rate
        let codec_guess = guess_codec(avg_packet_size, packets_per_sec);

        streams.push(RtpStream {
            flow,
            direction,
            packet_count: pkts.len(),
            duration_sec,
            packets_per_sec,
            jitter_avg_ms,
            jitter_max_ms,
            loss_pct,
            lost_packets,
            out_of_order,
            avg_packet_size,
            codec_guess,
        });
    }

    // Sort by packet count (most packets first)
    streams.sort_by(|a, b| b.packet_count.cmp(&a.packet_count));
    streams
}

/// Calculate jitter from packet timestamps.
fn calculate_jitter(packets: &[&UdpPacket]) -> (f64, f64) {
    if packets.len() < 2 {
        return (0.0, 0.0);
    }

    let mut jitters: Vec<f64> = Vec::new();
    let mut prev_ts = packets[0].timestamp;

    // Expected interval: ~20ms for most audio codecs
    let expected_interval = 0.020;

    for pkt in packets.iter().skip(1) {
        let interval = pkt.timestamp - prev_ts;
        let jitter = (interval - expected_interval).abs() * 1000.0; // Convert to ms

        // Only count reasonable jitter values (filter out huge gaps from silence)
        if jitter < 500.0 {
            jitters.push(jitter);
        }

        prev_ts = pkt.timestamp;
    }

    if jitters.is_empty() {
        return (0.0, 0.0);
    }

    let avg = jitters.iter().sum::<f64>() / jitters.len() as f64;
    let max = jitters.iter().cloned().fold(0.0, f64::max);

    (avg, max)
}

/// Analyze RTP sequence numbers for loss detection.
fn analyze_rtp_sequence(packets: &[&UdpPacket]) -> (f64, usize, usize) {
    if packets.len() < 2 {
        return (0.0, 0, 0);
    }

    let mut lost = 0usize;
    let mut out_of_order = 0usize;
    let mut prev_seq: Option<u16> = None;

    for pkt in packets {
        // Try to extract RTP sequence number (bytes 2-3 of RTP header)
        if pkt.payload.len() >= 4 {
            // Check RTP version (first 2 bits should be 2)
            let version = (pkt.payload[0] >> 6) & 0x03;
            if version == 2 {
                let seq = u16::from_be_bytes([pkt.payload[2], pkt.payload[3]]);

                if let Some(prev) = prev_seq {
                    let expected = prev.wrapping_add(1);
                    if seq != expected {
                        let gap = seq.wrapping_sub(prev);
                        if gap > 1 && gap < 100 {
                            lost += (gap - 1) as usize;
                        } else if gap > 65000 {
                            // Sequence went backwards
                            out_of_order += 1;
                        }
                    }
                }
                prev_seq = Some(seq);
            }
        }
    }

    let total_expected = packets.len() + lost;
    let loss_pct = if total_expected > 0 {
        (lost as f64 / total_expected as f64) * 100.0
    } else {
        0.0
    };

    (loss_pct, lost, out_of_order)
}

/// Guess the audio codec based on packet characteristics.
fn guess_codec(avg_size: usize, pps: f64) -> String {
    // Common codec characteristics:
    // - Opus: ~160 bytes at 50 pps (20ms frames)
    // - G.711: ~160 bytes at 50 pps
    // - G.729: ~20 bytes at 50 pps
    // - iLBC: ~38 bytes at 33 pps (30ms) or 50 bytes at 50 pps (20ms)

    if pps > 45.0 && pps < 55.0 {
        // 20ms frames
        if avg_size > 100 && avg_size < 200 {
            "Opus/G.711 (20ms)".to_string()
        } else if avg_size < 30 {
            "G.729 (20ms)".to_string()
        } else if avg_size >= 30 && avg_size <= 60 {
            "iLBC (20ms)".to_string()
        } else {
            format!("Unknown ({}B @ {:.0}pps)", avg_size, pps)
        }
    } else if pps > 30.0 && pps < 40.0 {
        // 30ms frames
        "iLBC/Opus (30ms)".to_string()
    } else {
        format!("Unknown ({}B @ {:.0}pps)", avg_size, pps)
    }
}

/// Calculate an overall quality score (0-100).
fn calculate_quality_score(streams: &[RtpStream], issues: &[String]) -> u8 {
    let mut score: f64 = 100.0;

    // Deduct for jitter
    for stream in streams {
        if stream.jitter_avg_ms > 10.0 {
            score -= (stream.jitter_avg_ms - 10.0).min(30.0);
        }
        if stream.jitter_max_ms > 50.0 {
            score -= ((stream.jitter_max_ms - 50.0) / 5.0).min(20.0);
        }

        // Deduct for packet loss (heavily penalized)
        score -= stream.loss_pct * 10.0;

        // Deduct for out-of-order
        score -= (stream.out_of_order as f64) * 0.5;
    }

    // Deduct for each issue
    score -= (issues.len() as f64) * 3.0;

    score.max(0.0).min(100.0) as u8
}

/// Try to parse SIP message from packet payload.
fn try_parse_sip(payload: &[u8], timestamp: f64) -> Option<SipMessage> {
    let text = std::str::from_utf8(payload).ok()?;

    // Look for SIP message start
    let first_line = text.lines().next()?;

    let method = if first_line.starts_with("SIP/2.0") {
        // Response: "SIP/2.0 200 OK"
        let parts: Vec<&str> = first_line.splitn(3, ' ').collect();
        if parts.len() >= 2 {
            if parts.len() >= 3 {
                format!("{} {}", parts[1], parts[2].split('\r').next().unwrap_or(parts[2]))
            } else {
                parts[1].to_string()
            }
        } else {
            return None;
        }
    } else if first_line.contains("SIP/2.0") {
        // Request: "INVITE sip:... SIP/2.0"
        first_line.split_whitespace().next()?.to_string()
    } else {
        return None;
    };

    let from = extract_sip_header(text, "From:")
        .map(|s| extract_phone_number(&s));
    let to = extract_sip_header(text, "To:")
        .map(|s| extract_phone_number(&s));
    let call_id = extract_sip_header(text, "Call-ID:");

    Some(SipMessage {
        timestamp_sec: timestamp,
        method,
        from,
        to,
        call_id,
    })
}

/// Extract a SIP header value.
fn extract_sip_header(text: &str, header: &str) -> Option<String> {
    for line in text.lines() {
        let line_lower = line.to_lowercase();
        let header_lower = header.to_lowercase();
        if line_lower.starts_with(&header_lower) {
            let value = line[header.len()..].trim();
            return Some(value.split(';').next().unwrap_or(value).trim().to_string());
        }
    }
    None
}

/// Extract phone number from SIP URI.
fn extract_phone_number(uri: &str) -> String {
    // Extract from formats like:
    // <sip:+16462826210@domain>
    // sip:+16462826210@domain
    if let Some(start) = uri.find(':') {
        let rest = &uri[start + 1..];
        if let Some(end) = rest.find('@') {
            return rest[..end].to_string();
        }
    }

    // Try to find a phone number pattern
    let digits: String = uri.chars()
        .filter(|c| c.is_ascii_digit() || *c == '+')
        .collect();

    if digits.len() >= 10 {
        digits
    } else {
        uri.to_string()
    }
}

/// Generate a text report from PCAP analysis.
pub fn generate_pcap_report(analysis: &PcapAnalysis) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push("# PCAP NETWORK ANALYSIS".to_string());
    lines.push(format!("packets={}", analysis.total_packets));
    lines.push(format!("duration_sec={:.2}", analysis.duration_sec));
    lines.push(format!("quality_score={}", analysis.quality_score));
    lines.push(String::new());

    // Call setup timing
    lines.push("## CALL SETUP".to_string());
    if let Some(setup) = analysis.call_setup_ms {
        lines.push(format!("call_setup_ms={:.0}", setup));
        let verdict = if setup < 2000.0 {
            "good"
        } else if setup < 5000.0 {
            "slow"
        } else {
            "very slow"
        };
        lines.push(format!("call_setup_verdict={}", verdict));
    }
    if let Some(media) = analysis.media_setup_ms {
        lines.push(format!("media_setup_ms={:.0}", media));
    }
    lines.push(String::new());

    // RTP Streams
    if !analysis.rtp_streams.is_empty() {
        lines.push("## RTP STREAMS".to_string());
        for (i, stream) in analysis.rtp_streams.iter().enumerate() {
            lines.push(format!("stream_{}_direction={}", i, stream.direction));
            lines.push(format!("stream_{}_packets={}", i, stream.packet_count));
            lines.push(format!("stream_{}_duration_sec={:.1}", i, stream.duration_sec));
            lines.push(format!("stream_{}_pps={:.1}", i, stream.packets_per_sec));
            lines.push(format!("stream_{}_jitter_avg_ms={:.1}", i, stream.jitter_avg_ms));
            lines.push(format!("stream_{}_jitter_max_ms={:.1}", i, stream.jitter_max_ms));
            lines.push(format!("stream_{}_loss_pct={:.2}", i, stream.loss_pct));
            lines.push(format!("stream_{}_lost_packets={}", i, stream.lost_packets));
            lines.push(format!("stream_{}_codec={}", i, stream.codec_guess));
            lines.push(String::new());
        }
    }

    // Issues
    if !analysis.issues.is_empty() {
        lines.push("## ISSUES DETECTED".to_string());
        for issue in &analysis.issues {
            lines.push(format!("issue: {}", issue));
        }
        lines.push(String::new());
    } else {
        lines.push("## NO ISSUES DETECTED".to_string());
        lines.push(String::new());
    }

    // SIP Timeline
    if !analysis.sip_messages.is_empty() {
        lines.push("## SIP SIGNALING TIMELINE".to_string());
        lines.push(format!("{:>8}  {:15}  {}", "TIME", "METHOD", "FROM -> TO"));
        lines.push("-".repeat(60));

        for msg in &analysis.sip_messages {
            let from_to = match (&msg.from, &msg.to) {
                (Some(f), Some(t)) => format!("{} -> {}", f, t),
                _ => String::new(),
            };
            lines.push(format!("{:>7.2}s  {:15}  {}", msg.timestamp_sec, msg.method, from_to));
        }
    }

    lines.join("\n")
}
