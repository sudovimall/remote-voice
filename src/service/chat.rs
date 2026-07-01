use crate::{
    Result,
    domain::room::{ChatMention, ChatMessage, RoomStore},
};
use std::sync::Arc;

/// 聊天服务负责把消息领域操作包装成信令层可直接发送的结果。
#[derive(Clone)]
pub struct ChatService {
    rooms: Arc<RoomStore>,
}

/// 聊天发送结果同时用于当前发送者确认和房间内其他成员广播。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatOutcome {
    pub request_id: Option<String>,
    pub message: ChatMessage,
}

impl ChatService {
    /// 创建聊天服务；消息校验仍复用 RoomStore，service 负责为信令层产出编排结果。
    pub fn new(rooms: Arc<RoomStore>) -> Self {
        Self { rooms }
    }

    /// 保存聊天消息并返回 ack/broadcast 共用的服务结果，避免 WebSocket 分支直接操作领域层。
    pub fn send_message(
        &self,
        room_id: &str,
        member_id: &str,
        content: &str,
        mentions: Vec<ChatMention>,
        request_id: Option<String>,
    ) -> Result<ChatOutcome> {
        let message = self
            .rooms
            .send_chat_message(room_id, member_id, content, mentions)?;
        Ok(ChatOutcome {
            request_id,
            message,
        })
    }

    /// 返回房间最近聊天历史，加入和恢复房间时用于构造 joined_room 响应。
    pub fn history(&self, room_id: &str) -> Result<Vec<ChatMessage>> {
        self.rooms.chat_history(room_id)
    }
}
