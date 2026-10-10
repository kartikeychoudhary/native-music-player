use protocol::BluetoothInfo;
use serde_json::Value;

fn props(obj: &Value) -> Option<&serde_json::Map<String, Value>> { obj.get("info")?.get("props")?.as_object() }

fn ldac_kbps(q: &str) -> Option<u32> { match q { "hq" => Some(990), "sq" => Some(660), "mq" => Some(330), _ => None } }

pub fn parse_pw_dump(json: &str, hint: &str) -> Option<BluetoothInfo> {
    let all: Vec<Value> = serde_json::from_str(json).ok()?;
    let sinks: Vec<&Value> = all.iter().filter(|o| {
        o.get("type").and_then(Value::as_str) == Some("PipeWire:Interface:Node")
            && props(o).is_some_and(|p| p.get("media.class").and_then(Value::as_str) == Some("Audio/Sink") && p.contains_key("api.bluez5.codec"))
    }).collect();
    let matches_hint = |o: &Value| props(o).is_some_and(|p| ["node.description", "node.nick", "node.name"].iter().any(|k| p.get(*k).and_then(Value::as_str).is_some_and(|v| !hint.is_empty() && !v.is_empty() && (v.contains(hint) || hint.contains(v)))));
    let node: &Value = sinks.iter().copied().find(|o| matches_hint(o)).or_else(|| (sinks.len() == 1).then(|| sinks[0]))?;
    let p = props(node)?;
    let codec = p.get("api.bluez5.codec").and_then(Value::as_str)?.to_ascii_lowercase();
    let device_id = p.get("device.id").and_then(Value::as_u64);
    let quality = p.get("bluez5.a2dp.ldac.quality").and_then(Value::as_str).map(str::to_owned).or_else(|| {
        let dev = all.iter().find(|o| o.get("id").and_then(Value::as_u64) == device_id && device_id.is_some())?;
        props(dev)?.get("bluez5.a2dp.ldac.quality").and_then(Value::as_str).map(str::to_owned)
    });
    let bitrate_kbps = if codec == "ldac" { quality.as_deref().and_then(ldac_kbps) } else { None };
    Some(BluetoothInfo { codec: Some(codec), quality, bitrate_kbps })
}

#[cfg(target_os = "linux")]
pub fn query(hint: &str) -> Option<BluetoothInfo> {
    let out = std::process::Command::new("pw-dump").output().ok()?;
    parse_pw_dump(&String::from_utf8_lossy(&out.stdout), hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DUMP: &str = r#"[
      {"id": 40, "type": "PipeWire:Interface:Device", "info": {"props": {"device.name": "bluez_card.AC_80_0A", "bluez5.a2dp.ldac.quality": "hq"}}},
      {"id": 55, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink", "node.name": "bluez_output.AC_80_0A.1", "node.description": "WH-1000XM5",
        "device.id": 40, "api.bluez5.codec": "ldac", "api.bluez5.profile": "a2dp-sink"}}},
      {"id": 60, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Audio/Sink", "node.name": "alsa_output.usb-dac", "node.description": "USB DAC"}}}
    ]"#;

    #[test]
    fn finds_ldac_with_quality_from_device_props() {
        let b = parse_pw_dump(DUMP, "WH-1000XM5").unwrap();
        assert_eq!(b.codec.as_deref(), Some("ldac"));
        assert_eq!(b.quality.as_deref(), Some("hq"));
        assert_eq!(b.bitrate_kbps, Some(990));
    }

    #[test]
    fn single_bluez_sink_matches_without_hint() {
        assert_eq!(parse_pw_dump(DUMP, "unrelated").unwrap().codec.as_deref(), Some("ldac"));
    }

    #[test]
    fn non_bluetooth_dump_is_none() {
        assert!(parse_pw_dump(r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Audio/Sink"}}}]"#, "x").is_none());
        assert!(parse_pw_dump("not json", "x").is_none());
    }
}
