use std::fmt;

use rusqlite::types::{FromSqlError, ValueRef};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid {0}: {1}")]
    InvalidValue(&'static str, String),
    #[error("unsupported schema version {0}; this build supports {1}")]
    UnsupportedSchemaVersion(u32, u32),
    #[error("graph depth {0} exceeds the maximum of {1}")]
    GraphDepthTooLarge(u32, u32),
}

pub type Result<T> = std::result::Result<T, Error>;

impl From<Error> for rusqlite::Error {
    fn from(value: Error) -> Self {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(value))
    }
}

macro_rules! string_id {
    ($name:ident, $label:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub struct $name(Uuid);

        impl $name {
            pub fn generate() -> Self {
                Self(Uuid::now_v7())
            }

            pub fn from_uuid(value: Uuid) -> Result<Self> {
                if value.get_version_num() != 7 {
                    return Err(Error::InvalidValue(
                        $label,
                        "identifier is not UUIDv7".to_owned(),
                    ));
                }
                Ok(Self(value))
            }

            pub fn as_uuid(&self) -> Uuid {
                self.0
            }

            pub fn as_str(&self) -> String {
                self.0.to_string()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl std::str::FromStr for $name {
            type Err = Error;

            fn from_str(value: &str) -> Result<Self> {
                let uuid = Uuid::parse_str(value)
                    .map_err(|_| Error::InvalidValue($label, value.to_owned()))?;
                Self::from_uuid(uuid)
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(
                &self,
                serializer: S,
            ) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(&self.0.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(
                deserializer: D,
            ) -> std::result::Result<Self, D::Error> {
                let value = String::deserialize(deserializer)?;
                value.parse().map_err(de::Error::custom)
            }
        }

        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::Owned(
                    rusqlite::types::Value::Text(self.0.to_string()),
                ))
            }
        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
                let text = value.as_str()?;
                text.parse()
                    .map_err(|error| FromSqlError::Other(Box::new(error)))
            }
        }
    };
}

string_id!(ResourceId, "resource id");
string_id!(AnchorId, "anchor id");
string_id!(KnowledgeCardId, "knowledge card id");
string_id!(NoteId, "note id");
string_id!(ExamSetId, "exam set id");
string_id!(QuestionGroupId, "question group id");
string_id!(QuestionCardId, "question card id");
string_id!(EdgeId, "edge id");
string_id!(ProposalId, "proposal id");
string_id!(UserId, "user id");
string_id!(AttemptId, "attempt id");
string_id!(CardId, "card id");
string_id!(JobId, "plugin job id");

impl From<KnowledgeCardId> for CardId {
    fn from(value: KnowledgeCardId) -> Self {
        Self(value.0)
    }
}

impl From<QuestionCardId> for CardId {
    fn from(value: QuestionCardId) -> Self {
        Self(value.0)
    }
}

macro_rules! string_enum {
    ($name:ident $(#[$doc:meta])* { $($variant:ident => $wire:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &'static [&'static str] = &[$($wire),+];

            pub const fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => $wire),+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = Error;

            fn from_str(value: &str) -> Result<Self> {
                match value {
                    $($wire => Ok(Self::$variant),)+
                    _ => Err(Error::InvalidValue(stringify!($name), value.to_owned())),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
                struct EnumVisitor;

                impl<'de> Visitor<'de> for EnumVisitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                        f.write_str(concat!("a ", stringify!($name), " value"))
                    }

                    fn visit_str<E: de::Error>(self, value: &str) -> std::result::Result<Self::Value, E> {
                        value.parse().map_err(E::custom)
                    }
                }

                deserializer.deserialize_str(EnumVisitor)
            }
        }

        impl rusqlite::ToSql for $name {
            fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
                Ok(rusqlite::types::ToSqlOutput::Owned(
                    rusqlite::types::Value::Text(self.as_str().to_owned()),
                ))
            }

        }

        impl rusqlite::types::FromSql for $name {
            fn column_result(value: ValueRef<'_>) -> std::result::Result<Self, FromSqlError> {
                let text = value.as_str()?;
                text.parse().map_err(|error| FromSqlError::Other(Box::new(error)))
            }
        }
    };
}

string_enum!(ResourceKind {
    Pdf => "pdf",
    Image => "image",
    Audio => "audio",
    Video => "video",
    Transcript => "transcript",
    Web => "web",
});

string_enum!(Sensitivity {
    Normal => "normal",
    Restricted => "restricted",
});

string_enum!(ExportPolicy {
    Allow => "allow",
    Redact => "redact",
    OwnerOnly => "owner_only",
});

string_enum!(AnchorKind {
    PdfPage => "pdf_page",
    Rect => "rect",
    MediaSpan => "media_span",
});

string_enum!(CardStatus {
    Draft => "draft",
    Staging => "staging",
    Published => "published",
    Archived => "archived",
});

string_enum!(Verification {
    Unchecked => "unchecked",
    Verified => "verified",
});

string_enum!(DiagramSyntax {
    Mermaid => "mermaid",
    Tikz => "tikz",
    Svg => "svg",
});

string_enum!(SourceKind {
    PastExam => "past_exam",
    QuestionBank => "question_bank",
    Original => "original",
    Custom => "custom",
});

string_enum!(QuestionType {
    SingleChoice => "single_choice",
    MultipleChoice => "multiple_choice",
    ShortAnswer => "short_answer",
    Vocab => "vocab",
    ImageQa => "image_qa",
    ImageOcclusion => "image_occlusion",
    PinIdentify => "pin_identify",
    PinLocate => "pin_locate",
});

string_enum!(GradingMode {
    Exact => "exact",
    AnyAccepted => "any_accepted",
    Voided => "voided",
});

string_enum!(AnswerSourceKind {
    Official => "official",
    PeerNote => "peer_note",
    DuplicateQuestion => "duplicate_question",
    Inferred => "inferred",
    OriginalAuthored => "original_authored",
    Unanswered => "unanswered",
});

string_enum!(EdgeType {
    Tests => "tests",
    ConfusableWith => "confusable_with",
    Prerequisite => "prerequisite",
    VariantOf => "variant_of",
});

string_enum!(EndpointKind {
    Card => "card",
    Question => "question",
    Option => "option",
});

string_enum!(ProposalStatus {
    Pending => "pending",
    Approved => "approved",
    Rejected => "rejected",
});

string_enum!(JobStatus {
    Queued => "queued",
    Running => "running",
    Succeeded => "succeeded",
    Failed => "failed",
    Cancelled => "cancelled",
});

string_enum!(ProposalTargetKind {
    Resource => "resource",
    KnowledgeCard => "knowledge_card",
    Note => "note",
    ExamSet => "exam_set",
    QuestionGroup => "question_group",
    QuestionCard => "question_card",
    Edge => "edge",
});

string_enum!(AttemptDirection {
    Default => "default",
    Forward => "forward",
    Reverse => "reverse",
});

string_enum!(SessionMode {
    DailyDue => "daily_due",
    FullExam => "full_exam",
    TopicDrill => "topic_drill",
    PracticalExam => "practical_exam",
    WeaknessDrill => "weakness_drill",
});

string_enum!(Confidence {
    Certain => "certain",
    Split => "split",
    Guessed => "guessed",
});

string_enum!(SchedulingState {
    New => "new",
    Learning => "learning",
    Review => "review",
    Relearning => "relearning",
});
