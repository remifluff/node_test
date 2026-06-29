use crate::parse::object_from_label;

#[derive(Clone, Debug, PartialEq)]
pub enum PdObject {
    OscTilde { freq: f32 },
    PlusTilde,
    MulTilde,
    DacTilde,
    Metro { ms: f32 },
    Random { max: i32 },
    FloatAtom { value: f32 },
    Message { text: String },
    Comment { text: String },
    In,
    Param,
    Out,
    DelayIn { id: Option<u8> },
    DelayOut { id: Option<u8> },
    Send { id: Option<u8> },
    Receive { id: Option<u8> },
    Combine,
}

impl PdObject {
    fn signal_label(kind: &str, id: Option<u8>) -> String {
        match id {
            Some(hex) => format!("{kind} #{hex:02X}"),
            None => kind.to_owned(),
        }
    }

    fn delay_label(kind: &str, id: Option<u8>) -> String {
        Self::signal_label(kind, id)
    }

    pub fn label(&self) -> String {
        match self {
            Self::OscTilde { freq } => format!("osc~ {freq}"),
            Self::PlusTilde => "+~".to_owned(),
            Self::MulTilde => "*~".to_owned(),
            Self::DacTilde => "dac~".to_owned(),
            Self::Metro { ms } => format!("metro {ms}"),
            Self::Random { max } => format!("random {max}"),
            Self::FloatAtom { value } => format!("{value:.3}"),
            Self::Message { text } => text.clone(),
            Self::Comment { text } => text.clone(),
            Self::In => "in".to_owned(),
            Self::Param => "param".to_owned(),
            Self::Out => "out".to_owned(),
            Self::DelayIn { id } => Self::delay_label("delay_in", *id),
            Self::DelayOut { id } => Self::delay_label("delay_out", *id),
            Self::Send { id } => Self::signal_label("send", *id),
            Self::Receive { id } => Self::signal_label("receive", *id),
            Self::Combine => "combine".to_owned(),
        }
    }

    pub fn bracketed_label(&self) -> String {
        match self {
            Self::Comment { text } => text.clone(),
            Self::Message { text } => format!("{text}"),
            Self::FloatAtom { .. } => self.label(),
            _ => format!("[{}]", self.label()),
        }
    }

    pub fn inlets(&self) -> usize {
        match self {
            Self::Comment { .. } | Self::Receive { .. } => 0,
            Self::In | Self::DelayIn { .. } => 0,
            Self::Combine => 2,
            Self::Send { .. } | Self::Out | Self::DelayOut { .. } => 1,
            _ => 1,
        }
    }

    pub fn outlets(&self) -> usize {
        match self {
            Self::Comment { .. } | Self::Send { .. } | Self::Out | Self::DacTilde => 0,
            Self::In | Self::Param | Self::DelayIn { .. } => 1,
            Self::Combine => 1,
            Self::Receive { .. } | Self::DelayOut { .. } => 1,
            _ => 1,
        }
    }

    pub fn is_comment(&self) -> bool {
        matches!(self, Self::Comment { .. })
    }

    pub fn is_send(&self) -> bool {
        matches!(self, Self::Send { .. })
    }

    pub fn is_receive(&self) -> bool {
        matches!(self, Self::Receive { .. })
    }

    pub fn signal_hex(&self) -> Option<u8> {
        match self {
            Self::DelayIn { id }
            | Self::DelayOut { id }
            | Self::Send { id }
            | Self::Receive { id } => *id,
            _ => None,
        }
    }

    pub fn is_number_box(&self) -> bool {
        matches!(self, Self::FloatAtom { .. })
    }

    pub fn edit_text(&self) -> String {
        match self {
            Self::Comment { text } | Self::Message { text } => text.clone(),
            Self::FloatAtom { value } => format!("{value}"),
            Self::OscTilde { freq } => format!("osc~ {freq}"),
            Self::Metro { ms } => format!("metro {ms}"),
            Self::Random { max } => format!("random {max}"),
            Self::PlusTilde => "+~".to_owned(),
            Self::MulTilde => "*~".to_owned(),
            Self::DacTilde => "dac~".to_owned(),
            Self::In => "in".to_owned(),
            Self::Param => "param".to_owned(),
            Self::Out => "out".to_owned(),
            Self::DelayIn { id } => Self::delay_label("delay_in", *id),
            Self::DelayOut { id } => Self::delay_label("delay_out", *id),
            Self::Send { id } => Self::signal_label("send", *id),
            Self::Receive { id } => Self::signal_label("receive", *id),
            Self::Combine => "combine".to_owned(),
        }
    }

    pub fn apply_edit_text(&mut self, text: &str) {
        *self = crate::parse::object_from_label(text);
    }

    /// Object label for `(node … :text …)` in `.lop` patch export.
    pub fn lop_text(&self, io_index: Option<usize>) -> String {
        match self {
            Self::In => format!("in {}", io_index.unwrap_or(1)),
            Self::Out => format!("out {}", io_index.unwrap_or(1)),
            Self::Param => format!("param {}", io_index.unwrap_or(1)),
            Self::PlusTilde => "+".to_owned(),
            Self::MulTilde => "*".to_owned(),
            Self::OscTilde { freq } => format!("osc~ {freq}"),
            Self::DacTilde => "dac~".to_owned(),
            Self::Metro { ms } => format!("metro {ms}"),
            Self::Random { max } => format!("random {max}"),
            Self::FloatAtom { value } => format!("{value}"),
            Self::Message { text } => text.clone(),
            Self::Comment { text } => text.clone(),
            Self::DelayIn { id } => Self::delay_label("delay_in", *id),
            Self::DelayOut { id } => Self::delay_label("delay_out", *id),
            Self::Send { id } => Self::signal_label("send", *id),
            Self::Receive { id } => Self::signal_label("receive", *id),
            Self::Combine => "combine".to_owned(),
        }
    }

    /// Optional `:bind` symbol for IO boxes in `.lop` patch export.
    pub fn lop_bind(&self, io_index: Option<usize>) -> Option<String> {
        match self {
            Self::In => Some(format!("_in_{}", io_index.unwrap_or(1))),
            Self::Out => Some(format!("_out_{}", io_index.unwrap_or(1))),
            Self::Param => Some(format!("_param_{}", io_index.unwrap_or(1))),
            _ => None,
        }
    }
}
