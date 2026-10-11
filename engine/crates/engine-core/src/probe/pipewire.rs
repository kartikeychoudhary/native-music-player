//! PipeWire graph facts from `pw-dump` JSON. The parsers are pure and target-independent so they
//! are unit-tested on every OS; only running `pw-dump` is Linux-specific.
use protocol::{BluetoothInfo, DeviceInfo, Transport};
use serde_json::{Map, Value};

type Props = Map<String, Value>;

fn props(obj: &Value) -> Option<&Props> { obj.get("info")?.get("props")?.as_object() }

fn str_prop<'a>(p: &'a Props, key: &str) -> Option<&'a str> { p.get(key).and_then(Value::as_str) }

/// PipeWire writes some integer props as numbers and others as strings.
fn u32_prop(p: &Props, key: &str) -> Option<u32> {
    match p.get(key)? {
        Value::Number(n) => n.as_u64().and_then(|v| u32::try_from(v).ok()),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

fn ldac_kbps(q: &str) -> Option<u32> { match q { "hq" => Some(990), "sq" => Some(660), "mq" => Some(330), _ => None } }

/// cpal's PipeWire host lists these synthetic nodes, which follow the default sink; they are not devices.
const SYNTHETIC_DEFAULTS: [&str; 2] = ["output_default", "sink_default"];
const ID_PREFIX: &str = "pipewire:";

/// The PipeWire node name behind a cpal device id (`pipewire:<node.name>`).
fn node_name_of(id: &str) -> Option<&str> { id.strip_prefix(ID_PREFIX) }

/// Whether `id` is one of cpal's synthetic "default" PipeWire devices.
pub fn is_synthetic_default(id: &str) -> bool { node_name_of(id).is_some_and(|n| SYNTHETIC_DEFAULTS.contains(&n)) }

/// Hides cpal's synthetic default devices and marks the real default sink (node name
/// `default_node`) as the default. With no known default sink, no device is marked.
pub fn apply_default(devices: Vec<DeviceInfo>, default_node: Option<&str>) -> Vec<DeviceInfo> {
    devices.into_iter().filter(|d| !is_synthetic_default(&d.id)).map(|mut d| {
        d.is_default = default_node.is_some_and(|n| node_name_of(&d.id) == Some(n));
        d
    }).collect()
}

/// The ALSA `hw:` device an `api.alsa.path` names, when it names exactly one. `hw:`/`plughw:` keep
/// their card and device; `front:<card>[,0]` is device 0 of that card by alsa-lib's card configs.
/// Other virtual PCMs (`hdmi:`, `surround51:`, `iec958:` ...) index their own tables, so they map
/// to `None` rather than a guess.
pub fn hw_from_alsa_path(path: &str) -> Option<String> {
    let (kind, args) = path.split_once(':')?;
    if args.is_empty() { return None; }
    match kind {
        "hw" | "plughw" => Some(format!("hw:{args}")),
        "front" => {
            let (card, dev) = args.split_once(',').unwrap_or((args, "0"));
            (card.parse::<u32>().is_ok() && dev == "0").then(|| format!("hw:{card},0"))
        }
        _ => None,
    }
}

/// The exact ALSA device a node plays to: `api.alsa.pcm.card`/`.device`, else `alsa.card`/`alsa.device`
/// (read back from the opened PCM), else `api.alsa.path`.
fn alsa_hw_of(p: &Props) -> Option<String> {
    let pair = |c: &str, d: &str| Some(format!("hw:{},{}", u32_prop(p, c)?, u32_prop(p, d)?));
    pair("api.alsa.pcm.card", "api.alsa.pcm.device")
        .or_else(|| pair("alsa.card", "alsa.device"))
        .or_else(|| str_prop(p, "api.alsa.path").and_then(hw_from_alsa_path))
}

/// Same mapping as cpal's PipeWire host (`device.api`, then `device.bus`), so a resolved default
/// sink reports the transport its own device-list entry has.
fn transport_of(p: &Props) -> Transport {
    if str_prop(p, "device.api") == Some("bluez5") { return Transport::Bluetooth; }
    if str_prop(p, "device.bus") == Some("usb") { Transport::Usb } else { Transport::Other }
}

/// An `Audio/Sink` node.
#[derive(Clone, Debug, PartialEq)]
pub struct Sink {
    pub name: String,
    pub description: String,
    pub transport: Transport,
    /// `hw:<card>,<device>` when the node is an ALSA PCM whose device is known.
    pub alsa_hw: Option<String>,
}

impl Sink {
    /// The cpal device id of this node.
    pub fn device_id(&self) -> String { format!("{ID_PREFIX}{}", self.name) }
}

/// A parsed `pw-dump`.
pub struct Dump(Vec<Value>);

impl Dump {
    pub fn parse(json: &str) -> Option<Dump> { serde_json::from_str(json).ok().map(Dump) }

    fn of_type<'a>(&'a self, t: &'a str) -> impl Iterator<Item = &'a Value> {
        self.0.iter().filter(move |o| o.get("type").and_then(Value::as_str) == Some(t))
    }

    fn sink_props(&self) -> impl Iterator<Item = &Props> {
        self.of_type("PipeWire:Interface:Node").filter_map(props).filter(|p| str_prop(p, "media.class") == Some("Audio/Sink"))
    }

    /// Node name of the default sink: key `default.audio.sink` of the `default` metadata, whose value
    /// is `{"name": "<node.name>"}` (an object, or the same JSON as a string).
    pub fn default_sink_name(&self) -> Option<String> {
        let meta = self.of_type("PipeWire:Interface:Metadata")
            .find(|o| o.get("props").and_then(|p| p.get("metadata.name")).and_then(Value::as_str) == Some("default"))?;
        let entry = meta.get("metadata")?.as_array()?.iter().find(|e| e.get("key").and_then(Value::as_str) == Some("default.audio.sink"))?;
        let value = match entry.get("value")? {
            Value::String(s) => serde_json::from_str(s).ok()?,
            v => v.clone(),
        };
        value.get("name")?.as_str().map(str::to_owned)
    }

    pub fn sink(&self, name: &str) -> Option<Sink> {
        let p = self.sink_props().find(|p| str_prop(p, "node.name") == Some(name))?;
        Some(Sink {
            name: name.to_owned(),
            description: str_prop(p, "node.description").unwrap_or(name).to_owned(),
            transport: transport_of(p),
            alsa_hw: alsa_hw_of(p),
        })
    }

    pub fn default_sink(&self) -> Option<Sink> { self.sink(&self.default_sink_name()?) }

    /// The sink a device id plays to: the default sink for `None` and cpal's synthetic defaults.
    fn sink_for(&self, id: Option<&str>) -> Result<Sink, String> {
        let name = match id {
            None => None,
            Some(id) if is_synthetic_default(id) => None,
            Some(id) => Some(node_name_of(id).ok_or_else(|| format!("\"{id}\" is not a PipeWire device"))?.to_owned()),
        };
        let name = match name { Some(n) => n, None => self.default_sink_name().ok_or("PipeWire reports no default sink")? };
        self.sink(&name).ok_or_else(|| format!("PipeWire sink \"{name}\" not found"))
    }

    /// The ALSA `hw:<card>,<device>` that device `id` (`None` = system default) plays to, or why it
    /// can't be determined.
    pub fn hw_device_for(&self, id: Option<&str>) -> Result<String, String> {
        let sink = self.sink_for(id)?;
        sink.alsa_hw.ok_or_else(|| format!("can't tell which ALSA device \"{}\" uses", sink.description))
    }

    /// Codec details of the Bluetooth sink matching `hint` (a device name), or of the only one.
    pub fn bluetooth(&self, hint: &str) -> Option<BluetoothInfo> {
        let sinks: Vec<&Props> = self.sink_props().filter(|p| p.contains_key("api.bluez5.codec")).collect();
        let matches_hint = |p: &Props| ["node.description", "node.nick", "node.name"].iter().any(|k| str_prop(p, k).is_some_and(|v| !hint.is_empty() && !v.is_empty() && (v.contains(hint) || hint.contains(v))));
        let p = sinks.iter().copied().find(|p| matches_hint(p)).or_else(|| (sinks.len() == 1).then(|| sinks[0]))?;
        let codec = str_prop(p, "api.bluez5.codec")?.to_ascii_lowercase();
        let device_id = p.get("device.id").and_then(Value::as_u64);
        let quality = str_prop(p, "bluez5.a2dp.ldac.quality").map(str::to_owned).or_else(|| {
            let dev = self.0.iter().find(|o| device_id.is_some() && o.get("id").and_then(Value::as_u64) == device_id)?;
            str_prop(props(dev)?, "bluez5.a2dp.ldac.quality").map(str::to_owned)
        });
        let bitrate_kbps = if codec == "ldac" { quality.as_deref().and_then(ldac_kbps) } else { None };
        Some(BluetoothInfo { codec: Some(codec), quality, bitrate_kbps })
    }
}

pub fn parse_pw_dump(json: &str, hint: &str) -> Option<BluetoothInfo> { Dump::parse(json)?.bluetooth(hint) }

/// Runs `cmd` and returns its stdout if it exits successfully within `timeout`; otherwise kills it.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) fn output_within(mut cmd: std::process::Command, timeout: std::time::Duration) -> Option<String> {
    use std::io::Read;
    use std::process::Stdio;
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut stdout = child.stdout.take().expect("stdout is piped");
    // Read on another thread: a dump larger than the pipe buffer would otherwise block the child.
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        stdout.read_to_string(&mut out).map(|_| out)
    });
    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if std::time::Instant::now() < deadline => std::thread::sleep(std::time::Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None; // the reader ends when the killed child's pipe closes
            }
        }
    };
    let out = reader.join().ok()?.ok()?;
    status.success().then_some(out)
}

/// The current PipeWire graph; `None` without PipeWire, or if `pw-dump` fails or takes over 1 s.
#[cfg(target_os = "linux")]
pub fn dump() -> Option<Dump> {
    Dump::parse(&output_within(std::process::Command::new("pw-dump"), std::time::Duration::from_secs(1))?)
}

#[cfg(target_os = "linux")]
pub fn query(hint: &str) -> Option<BluetoothInfo> { dump()?.bluetooth(hint) }

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{DeviceInfo, Transport};

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

    /// A trimmed real-world dump: the default-sink metadata, an HDA card with analog and HDMI
    /// sinks, a USB DAC whose props carry only `api.alsa.path`, and a Bluetooth sink.
    const SYSTEM: &str = r#"[
      {"id": 31, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "settings"},
       "metadata": [{"subject": 0, "key": "clock.rate", "type": "", "value": 48000}]},
      {"id": 32, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
       "metadata": [
         {"subject": 0, "key": "default.configured.audio.sink", "type": "Spa:String:JSON", "value": {"name": "alsa_output.usb-dac"}},
         {"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON", "value": {"name": "alsa_output.pci-0000_00_1f.3.hdmi-stereo"}},
         {"subject": 0, "key": "default.audio.source", "type": "Spa:String:JSON", "value": {"name": "alsa_input.pci"}}
       ]},
      {"id": 50, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink", "node.name": "alsa_output.pci-0000_00_1f.3.analog-stereo", "node.description": "Built-in Audio Analog Stereo",
        "device.api": "alsa", "device.bus": "pci", "api.alsa.path": "front:0", "api.alsa.pcm.card": "0", "api.alsa.pcm.device": "0"}}},
      {"id": 51, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink", "node.name": "alsa_output.pci-0000_00_1f.3.hdmi-stereo", "node.description": "Built-in Audio Digital Stereo (HDMI)",
        "device.api": "alsa", "device.bus": "pci", "api.alsa.path": "hdmi:0", "alsa.card": 0, "alsa.device": 3}}},
      {"id": 60, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink", "node.name": "alsa_output.usb-dac", "node.description": "USB DAC",
        "device.api": "alsa", "device.bus": "usb", "api.alsa.path": "hw:2,1"}}},
      {"id": 61, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink", "node.name": "alsa_output.surround", "node.description": "Surround", "device.api": "alsa", "api.alsa.path": "surround51:0"}}},
      {"id": 70, "type": "PipeWire:Interface:Node", "info": {"props": {
        "media.class": "Audio/Sink", "node.name": "bluez_output.AC_80_0A.1", "node.description": "WH-1000XM5",
        "device.api": "bluez5", "api.bluez5.codec": "aac"}}},
      {"id": 80, "type": "PipeWire:Interface:Node", "info": {"props": {"media.class": "Stream/Output/Audio", "node.name": "Firefox"}}}
    ]"#;

    fn dump() -> Dump { Dump::parse(SYSTEM).unwrap() }

    #[test]
    fn default_sink_comes_from_the_default_metadata() {
        let d = dump();
        assert_eq!(d.default_sink_name().as_deref(), Some("alsa_output.pci-0000_00_1f.3.hdmi-stereo"));
        let sink = d.default_sink().unwrap();
        assert_eq!(sink.description, "Built-in Audio Digital Stereo (HDMI)");
        assert_eq!(sink.transport, Transport::Other);
    }

    #[test]
    fn default_sink_value_may_be_a_json_string() {
        let d = Dump::parse(r#"[{"id": 1, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
            "metadata": [{"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON", "value": "{\"name\": \"x\"}"}]}]"#).unwrap();
        assert_eq!(d.default_sink_name().as_deref(), Some("x"));
    }

    #[test]
    fn no_default_metadata_means_no_default_sink() {
        assert_eq!(Dump::parse(DUMP).unwrap().default_sink_name(), None);
        assert!(Dump::parse("{}").is_none());
    }

    #[test]
    fn sink_transport_follows_device_api_and_bus() {
        let d = dump();
        assert_eq!(d.sink("bluez_output.AC_80_0A.1").unwrap().transport, Transport::Bluetooth);
        assert_eq!(d.sink("alsa_output.usb-dac").unwrap().transport, Transport::Usb);
        assert!(d.sink("Firefox").is_none(), "streams are not sinks");
        assert!(d.sink("missing").is_none());
    }

    #[test]
    fn alsa_hw_device_from_pcm_props_alsa_props_or_path() {
        let d = dump();
        assert_eq!(d.sink("alsa_output.pci-0000_00_1f.3.analog-stereo").unwrap().alsa_hw.as_deref(), Some("hw:0,0"));
        assert_eq!(d.sink("alsa_output.pci-0000_00_1f.3.hdmi-stereo").unwrap().alsa_hw.as_deref(), Some("hw:0,3"));
        assert_eq!(d.sink("alsa_output.usb-dac").unwrap().alsa_hw.as_deref(), Some("hw:2,1"));
        assert_eq!(d.sink("alsa_output.surround").unwrap().alsa_hw, None);
        assert_eq!(d.sink("bluez_output.AC_80_0A.1").unwrap().alsa_hw, None);
    }

    #[test]
    fn alsa_paths() {
        assert_eq!(hw_from_alsa_path("hw:0,3").as_deref(), Some("hw:0,3"));
        assert_eq!(hw_from_alsa_path("plughw:1,2").as_deref(), Some("hw:1,2"));
        assert_eq!(hw_from_alsa_path("hw:CARD=D50s,DEV=0").as_deref(), Some("hw:CARD=D50s,DEV=0"));
        assert_eq!(hw_from_alsa_path("front:0").as_deref(), Some("hw:0,0"));
        assert_eq!(hw_from_alsa_path("front:1,0").as_deref(), Some("hw:1,0"));
        assert_eq!(hw_from_alsa_path("front:1,1"), None);
        assert_eq!(hw_from_alsa_path("hdmi:0,1"), None, "HDMI indices are not hw device numbers");
        assert_eq!(hw_from_alsa_path("hw:"), None);
        assert_eq!(hw_from_alsa_path("default"), None);
    }

    #[test]
    fn hw_device_for_a_device_id() {
        let d = dump();
        assert_eq!(d.hw_device_for(None), Ok("hw:0,3".into()), "system default resolves through the metadata");
        assert_eq!(d.hw_device_for(Some("pipewire:output_default")), Ok("hw:0,3".into()));
        assert_eq!(d.hw_device_for(Some("pipewire:alsa_output.usb-dac")), Ok("hw:2,1".into()));
        let surround = d.hw_device_for(Some("pipewire:alsa_output.surround")).unwrap_err();
        assert!(surround.contains("Surround"), "{surround}");
        assert!(d.hw_device_for(Some("pipewire:missing")).is_err());
        assert!(Dump::parse(DUMP).unwrap().hw_device_for(None).is_err(), "no default sink");
    }

    fn dev(id: &str, is_default: bool) -> DeviceInfo {
        DeviceInfo { id: id.into(), name: id.into(), transport: Transport::Other, is_default, sample_rates: vec![], exclusive_capable: true }
    }

    #[test]
    fn device_list_hides_synthetic_defaults_and_marks_the_real_one() {
        let list = vec![
            dev("pipewire:sink_default", false), dev("pipewire:output_default", true),
            dev("pipewire:alsa_output.usb-dac", false), dev("pipewire:alsa_output.pci-0000_00_1f.3.hdmi-stereo", false),
        ];
        let out = apply_default(list.clone(), Some("alsa_output.pci-0000_00_1f.3.hdmi-stereo"));
        assert_eq!(out.iter().map(|d| (d.id.as_str(), d.is_default)).collect::<Vec<_>>(), vec![
            ("pipewire:alsa_output.usb-dac", false), ("pipewire:alsa_output.pci-0000_00_1f.3.hdmi-stereo", true),
        ]);
        let unknown = apply_default(list, None);
        assert_eq!(unknown.len(), 2);
        assert!(unknown.iter().all(|d| !d.is_default));
    }

    #[test]
    fn synthetic_ids() {
        assert!(is_synthetic_default("pipewire:output_default"));
        assert!(is_synthetic_default("pipewire:sink_default"));
        assert!(!is_synthetic_default("pipewire:alsa_output.usb-dac"));
    }

    #[test]
    fn non_bluetooth_dump_is_none() {
        assert!(parse_pw_dump(r#"[{"id":1,"type":"PipeWire:Interface:Node","info":{"props":{"media.class":"Audio/Sink"}}}]"#, "x").is_none());
        assert!(parse_pw_dump("not json", "x").is_none());
    }

    fn shell(script: &str) -> std::process::Command {
        let mut c = if cfg!(windows) { std::process::Command::new("cmd") } else { std::process::Command::new("sh") };
        c.args(if cfg!(windows) { ["/C", script] } else { ["-c", script] });
        c
    }

    #[test]
    fn command_output_is_returned_on_success() {
        assert_eq!(output_within(shell("echo hi"), std::time::Duration::from_secs(10)).map(|s| s.trim().to_owned()).as_deref(), Some("hi"));
    }

    #[test]
    fn failing_or_missing_commands_give_none() {
        assert_eq!(output_within(shell("exit 3"), std::time::Duration::from_secs(10)), None);
        assert_eq!(output_within(std::process::Command::new("aurora-no-such-binary"), std::time::Duration::from_secs(10)), None);
    }

    #[test]
    fn slow_commands_are_killed_at_the_timeout() {
        let slow = if cfg!(windows) { "ping -n 6 127.0.0.1 >NUL" } else { "sleep 5" };
        let start = std::time::Instant::now();
        assert_eq!(output_within(shell(slow), std::time::Duration::from_millis(200)), None);
        assert!(start.elapsed() < std::time::Duration::from_secs(3), "{:?}", start.elapsed());
    }
}
