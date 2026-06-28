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
    #[error("恢复凭据无效")]
    InvalidResumeToken,
    #[error("请先登录")]
    Unauthenticated,
    #[error("没有权限执行该操作")]
    Forbidden,
    #[error("用户名或密码错误")]
    InvalidCredentials,
    #[error("邀请码不存在")]
    InviteNotFound,
    #[error("邀请码已过期")]
    InviteExpired,
    #[error("邀请码已被使用")]
    InviteUsed,
    #[error("用户名已被占用")]
    UsernameTaken,
    #[error("登录状态已过期")]
    SessionExpired,
    #[error("认证系统未启用")]
    AuthDisabled,
    #[error("房间已关闭")]
    RoomClosed,
    #[error("媒体层尚未就绪")]
    MediaNotReady,
    #[error("消息格式无效: {0}")]
    InvalidMessage(String),
    #[error("配置解析失败: {0}")]
    Config(#[from] serde_yaml::Error),
    #[error("配置无效: {0}")]
    ConfigValue(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
    #[error("数据库错误: {0}")]
    Database(String),
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
            Error::Unauthenticated | Error::SessionExpired => StatusCode::UNAUTHORIZED,
            Error::NotRoomOwner | Error::InvalidResumeToken | Error::Forbidden => {
                StatusCode::FORBIDDEN
            }
            Error::InvalidCredentials | Error::InviteNotFound => StatusCode::UNAUTHORIZED,
            Error::InviteExpired | Error::InviteUsed | Error::UsernameTaken | Error::RoomClosed => {
                StatusCode::CONFLICT
            }
            Error::AuthDisabled => StatusCode::NOT_FOUND,
            Error::MediaNotReady => StatusCode::SERVICE_UNAVAILABLE,
            Error::InvalidMessage(_) | Error::Config(_) | Error::ConfigValue(_) => {
                StatusCode::BAD_REQUEST
            }
            Error::Io(_) | Error::Database(_) | Error::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        }
    }

    pub fn code(&self) -> &'static str {
        match self {
            Error::RoomNotFound => "room_not_found",
            Error::RoomFull => "room_full",
            Error::NotRoomOwner => "not_room_owner",
            Error::MemberNotFound => "member_not_found",
            Error::InvalidResumeToken => "invalid_resume_token",
            Error::Unauthenticated => "unauthenticated",
            Error::Forbidden => "forbidden",
            Error::InvalidCredentials => "invalid_credentials",
            Error::InviteNotFound => "invite_not_found",
            Error::InviteExpired => "invite_expired",
            Error::InviteUsed => "invite_used",
            Error::UsernameTaken => "username_taken",
            Error::SessionExpired => "session_expired",
            Error::AuthDisabled => "auth_disabled",
            Error::RoomClosed => "room_closed",
            Error::MediaNotReady => "media_not_ready",
            Error::InvalidMessage(_) => "invalid_message",
            Error::Config(_) | Error::ConfigValue(_) => "config_error",
            Error::Io(_) | Error::Database(_) | Error::Internal(_) => "internal_error",
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
