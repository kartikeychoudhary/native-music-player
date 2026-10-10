#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceSample { I16, I24, I32, F32 }

impl DeviceSample {
    pub fn bytes(self) -> usize { match self { Self::I16 => 2, Self::I24 => 3, Self::I32 | Self::F32 => 4 } }
    pub fn bits(self) -> u16 { match self { Self::I16 => 16, Self::I24 => 24, Self::I32 | Self::F32 => 32 } }
    pub fn is_int(self) -> bool { !matches!(self, Self::F32) }
    pub fn wire_name(self) -> &'static str { match self { Self::I16 => "s16", Self::I24 => "s24", Self::I32 => "s32", Self::F32 => "f32" } }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OutputFormat { pub sample_rate: u32, pub channels: u16, pub sample: DeviceSample }

impl OutputFormat {
    pub fn bytes_per_frame(&self) -> usize { self.sample.bytes() * self.channels as usize }
}
