use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("房间不存在")]
    RoomNotFound,
    #[error("房间人数已满")]
    RoomFull,
    #[error("只有房主可以执行该操作")]
    NotRoomOwner,
    #[error("成员不存在")]
    MemberNotFound,
    #[error("媒体层尚未就绪")]
    MediaNotReady,
    #[error("消息格式无效: {0}")]
    InvalidMessage(String),
    #[error("配置解析失败: {0}")]
    Config(#[from] serde_yaml::Error),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("内部错误: {0}")]
    Internal(String),
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Error::RoomNotFound | Error::MemberNotFound => StatusCode::NOT_FOUND,
            Error::RoomFull => StatusCode::CONFLICT,
            Error::NotRoomOwner => StatusCode::FORBIDDEN,
            Error::MediaNotReady => StatusCode::SERVICE_UNAVAILABLE,
            Error::InvalidMessage(_) | Error::Config(_) => StatusCode::BAD_REQUEST,
            Error::Io(_) | Error::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Error::RoomNotFound => "room_not_found",
            Error::RoomFull => "room_full",
            Error::NotRoomOwner => "not_room_owner",
            Error::MemberNotFound => "member_not_found",
            Error::MediaNotReady => "media_not_ready",
            Error::InvalidMessage(_) => "invalid_message",
            Error::Config(_) => "config_error",
            Error::Io(_) | Error::Internal(_) => "internal_error",
        }
    }
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = ErrorBody {
            code: self.code(),
            message: self.to_string(),
        };
        (status, Json(body)).into_response()
    }
}
