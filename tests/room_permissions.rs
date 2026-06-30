use voice::Error;
use voice::domain::room::{MediaRoute, RoomJoin, RoomStore};

// 构建三人房间，便于媒体路由测试验证单个成员对不会影响其他成员对。
fn three_member_room() -> (RoomStore, RoomJoin, RoomJoin, RoomJoin) {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let first = store.join_room(&owner.room.id, "一号").expect("一号加入");
    let second = store.join_room(&owner.room.id, "二号").expect("二号加入");

    (store, owner, first, second)
}

// 断言指定成员对的媒体路由，统一覆盖正向和反向读取时的期望。
fn assert_media_route(
    store: &RoomStore,
    room_id: &str,
    first_member_id: &str,
    second_member_id: &str,
    expected: MediaRoute,
) {
    assert_eq!(
        store
            .media_route(room_id, first_member_id, second_member_id)
            .expect("读取媒体路由"),
        expected
    );
}

#[test]
fn 房主可以关闭成员发言权限() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");
    let member = store
        .join_room(&owner.room.id, "队友")
        .expect("成员加入成功");

    let room = store
        .set_member_can_speak(&owner.room.id, &owner.member.id, &member.member.id, false)
        .expect("房主可以修改成员权限");

    assert!(!room.members[&member.member.id].can_speak);
}

#[test]
fn 普通成员不能修改他人发言权限() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");
    let member = store
        .join_room(&owner.room.id, "队友")
        .expect("成员加入成功");

    let err = store
        .set_member_can_speak(&owner.room.id, &member.member.id, &owner.member.id, false)
        .expect_err("普通成员不能修改权限");

    assert!(matches!(err, Error::NotRoomOwner));
}

#[test]
fn 房间满员后拒绝加入() {
    let store = RoomStore::new(1);
    let owner = store.create_room("房主").expect("创建房间成功");

    let err = store
        .join_room(&owner.room.id, "第二个人")
        .expect_err("超过人数上限应失败");

    assert!(matches!(err, Error::RoomFull));
}

#[test]
fn 房间_id_不是简单连续编号() {
    let store = RoomStore::new(8);

    let first = store.create_room("房主 1").expect("创建第一个房间");
    let second = store.create_room("房主 2").expect("创建第二个房间");

    assert_ne!(first.room.id, "000001");
    assert_ne!(second.room.id, "000002");
}

#[test]
fn 成员_id_不是简单连续编号() {
    let store = RoomStore::new(8);

    let owner = store.create_room("房主").expect("创建房间成功");
    let member = store
        .join_room(&owner.room.id, "队友")
        .expect("成员加入成功");

    assert_ne!(owner.member.id, "m1");
    assert_ne!(member.member.id, "m2");
}

#[test]
fn 成员可以更新自己的本地静音状态() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");

    let room = store
        .set_self_muted(&owner.room.id, &owner.member.id, true)
        .expect("成员可以更新自己的静音状态");

    assert!(room.members[&owner.member.id].self_muted);
}

#[test]
fn 成员可以停止并恢复接收另一成员语音() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    let blocked = store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, false)
        .expect("成员可以不听另一成员");
    assert_eq!(
        blocked.not_listening_member_ids,
        vec![member.member.id.clone()]
    );

    let listening = store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, true)
        .expect("成员可以恢复接收");
    assert!(listening.not_listening_member_ids.is_empty());
}

#[test]
fn 同房间只能一个成员共享屏幕() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let first = store.join_room(&owner.room.id, "共享者").expect("加入房间");
    let second = store.join_room(&owner.room.id, "观众").expect("加入房间");

    let room = store
        .start_screen_share(&owner.room.id, &first.member.id)
        .expect("第一个成员可以共享屏幕");
    assert_eq!(
        room.screen_share
            .as_ref()
            .map(|share| share.member_id.as_str()),
        Some(first.member.id.as_str())
    );
    assert_eq!(
        room.screen_share
            .as_ref()
            .map(|share| share.nickname.as_str()),
        Some("共享者")
    );

    let error = store
        .start_screen_share(&owner.room.id, &second.member.id)
        .expect_err("第二个成员不能同时共享屏幕");
    assert!(matches!(error, Error::InvalidMessage(_)));
}

#[test]
fn 房主可以强制停止成员屏幕共享() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    store
        .start_screen_share(&owner.room.id, &member.member.id)
        .expect("成员开始共享");
    let room = store
        .stop_screen_share(&owner.room.id, &owner.member.id)
        .expect("房主可以强制停止");

    assert!(room.screen_share.is_none());
}

#[test]
fn 普通成员不能停止别人屏幕共享() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let sharer = store.join_room(&owner.room.id, "共享者").expect("加入房间");
    let viewer = store.join_room(&owner.room.id, "观众").expect("加入房间");

    store
        .start_screen_share(&owner.room.id, &sharer.member.id)
        .expect("成员开始共享");
    let error = store
        .stop_screen_share(&owner.room.id, &viewer.member.id)
        .expect_err("普通成员不能停止别人共享");

    assert!(matches!(error, Error::NotRoomOwner));
}

#[test]
fn 共享者离开或断线过期后释放共享占用() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let leaver = store.join_room(&owner.room.id, "离开者").expect("加入房间");

    store
        .start_screen_share(&owner.room.id, &leaver.member.id)
        .expect("成员开始共享");
    let room = store
        .leave_room(&owner.room.id, &leaver.member.id)
        .expect("共享者离开");
    assert!(room.screen_share.is_none());

    let expiring = store.join_room(&owner.room.id, "断线者").expect("加入房间");
    store
        .start_screen_share(&owner.room.id, &expiring.member.id)
        .expect("成员再次共享");
    store
        .mark_member_disconnected(&owner.room.id, &expiring.member.id)
        .expect("共享者断线");
    let room = store
        .expire_disconnected_member(&owner.room.id, &expiring.member.id)
        .expect("断线共享者超时清理")
        .expect("断线共享者被移除");
    assert!(room.screen_share.is_none());
}

#[test]
fn 多个成员可以开启并幂等关闭摄像头() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let first = store.join_room(&owner.room.id, "一号").expect("加入房间");
    let second = store.join_room(&owner.room.id, "二号").expect("加入房间");

    let room = store
        .start_video_call(&owner.room.id, &first.member.id)
        .expect("一号开启摄像头");
    assert_eq!(room.video_call_publishers.len(), 1);
    assert_eq!(
        room.video_call_publishers[&first.member.id].nickname,
        "一号"
    );

    let room = store
        .start_video_call(&owner.room.id, &first.member.id)
        .expect("重复开启摄像头保持幂等");
    assert_eq!(room.video_call_publishers.len(), 1);

    let room = store
        .start_video_call(&owner.room.id, &second.member.id)
        .expect("二号也可以开启摄像头");
    assert_eq!(room.video_call_publishers.len(), 2);

    let room = store
        .stop_video_call(&owner.room.id, &first.member.id)
        .expect("一号关闭摄像头");
    assert!(!room.video_call_publishers.contains_key(&first.member.id));
    assert!(room.video_call_publishers.contains_key(&second.member.id));

    let room = store
        .stop_video_call(&owner.room.id, &first.member.id)
        .expect("重复关闭摄像头保持幂等");
    assert_eq!(room.video_call_publishers.len(), 1);
}

#[test]
fn 离线成员不能开启摄像头且断线离开会释放摄像头状态() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let leaver = store.join_room(&owner.room.id, "离开者").expect("加入房间");

    store
        .start_video_call(&owner.room.id, &leaver.member.id)
        .expect("成员开启摄像头");
    let room = store
        .mark_member_disconnected(&owner.room.id, &leaver.member.id)
        .expect("成员断线");
    assert!(!room.video_call_publishers.contains_key(&leaver.member.id));
    let error = store
        .start_video_call(&owner.room.id, &leaver.member.id)
        .expect_err("离线成员不能开启摄像头");
    assert!(matches!(error, Error::InvalidMessage(_)));

    store
        .resume_room(&owner.room.id, &leaver.member.id, &leaver.resume_token)
        .expect("成员恢复连接");
    store
        .start_video_call(&owner.room.id, &leaver.member.id)
        .expect("恢复后重新开启摄像头");
    let room = store
        .leave_room(&owner.room.id, &leaver.member.id)
        .expect("成员离开");
    assert!(!room.video_call_publishers.contains_key(&leaver.member.id));
}

#[test]
fn 未见过的成员对默认使用_p2p_路由() {
    let (store, owner, first, _second) = three_member_room();

    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &first.member.id,
        MediaRoute::P2p,
    );
}

#[test]
fn p2p_失败会归一化成员对并只回退这一对() {
    let (store, owner, first, second) = three_member_room();

    let update = store
        .mark_p2p_connection_failed(&owner.room.id, &first.member.id, &owner.member.id)
        .expect("标记 P2P 失败");

    let mut expected_pair = vec![owner.member.id.clone(), first.member.id.clone()];
    expected_pair.sort();
    assert_eq!(update.member_ids, expected_pair);
    assert_eq!(update.route, MediaRoute::Sfu);
    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &first.member.id,
        MediaRoute::Sfu,
    );
    assert_media_route(
        &store,
        &owner.room.id,
        &first.member.id,
        &owner.member.id,
        MediaRoute::Sfu,
    );
    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &second.member.id,
        MediaRoute::P2p,
    );
    assert_media_route(
        &store,
        &owner.room.id,
        &first.member.id,
        &second.member.id,
        MediaRoute::P2p,
    );
}

#[test]
fn p2p_目标必须是同房间在线的其他成员() {
    let (store, owner, first, _second) = three_member_room();
    let other_room = store.create_room("其他房主").expect("创建其他房间");

    let self_error = store
        .validate_p2p_target(&owner.room.id, &owner.member.id, &owner.member.id)
        .expect_err("不能向自己发送 P2P 信令");
    assert!(matches!(self_error, Error::InvalidMessage(_)));

    let missing_error = store
        .validate_p2p_target(&owner.room.id, &owner.member.id, "m_missing")
        .expect_err("不能向不存在成员发送 P2P 信令");
    assert!(matches!(missing_error, Error::MemberNotFound));

    let cross_room_error = store
        .validate_p2p_target(&owner.room.id, &owner.member.id, &other_room.member.id)
        .expect_err("不能跨房间发送 P2P 信令");
    assert!(matches!(cross_room_error, Error::MemberNotFound));

    store
        .mark_member_disconnected(&owner.room.id, &first.member.id)
        .expect("成员断线");
    let offline_error = store
        .validate_p2p_target(&owner.room.id, &owner.member.id, &first.member.id)
        .expect_err("不能向离线成员发送 P2P 信令");
    assert!(matches!(offline_error, Error::InvalidMessage(_)));
}

#[test]
fn 可恢复断线期间保留媒体路由_过期后清理() {
    let (store, owner, first, _second) = three_member_room();

    store
        .mark_p2p_connection_failed(&owner.room.id, &owner.member.id, &first.member.id)
        .expect("标记 P2P 失败");
    store
        .mark_member_disconnected(&owner.room.id, &first.member.id)
        .expect("成员断线");
    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &first.member.id,
        MediaRoute::Sfu,
    );

    store
        .resume_room(&owner.room.id, &first.member.id, &first.resume_token)
        .expect("成员恢复");
    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &first.member.id,
        MediaRoute::Sfu,
    );

    store
        .mark_member_disconnected(&owner.room.id, &first.member.id)
        .expect("成员再次断线");
    store
        .expire_disconnected_member(&owner.room.id, &first.member.id)
        .expect("断线成员过期")
        .expect("成员被移除");
    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &first.member.id,
        MediaRoute::P2p,
    );
}

#[test]
fn 普通成员离开后只清理相关媒体路由() {
    let (store, owner, first, second) = three_member_room();

    store
        .mark_p2p_connection_failed(&owner.room.id, &owner.member.id, &first.member.id)
        .expect("标记第一对 P2P 失败");
    store
        .mark_p2p_connection_failed(&owner.room.id, &owner.member.id, &second.member.id)
        .expect("标记第二对 P2P 失败");
    store
        .leave_room(&owner.room.id, &first.member.id)
        .expect("成员离开");

    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &first.member.id,
        MediaRoute::P2p,
    );
    assert_media_route(
        &store,
        &owner.room.id,
        &owner.member.id,
        &second.member.id,
        MediaRoute::Sfu,
    );
}

#[test]
fn 成员恢复原身份后保留不听名单() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, false)
        .expect("写入不听名单");
    store
        .mark_member_disconnected(&owner.room.id, &owner.member.id)
        .expect("房主断线");

    let resumed = store
        .resume_room(&owner.room.id, &owner.member.id, &owner.resume_token)
        .expect("恢复房间");
    assert_eq!(
        resumed.member.not_listening_member_ids(),
        vec![member.member.id.clone()]
    );
}

#[test]
fn 成员不能屏蔽自己且目标离开后清理不听引用() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    let error = store
        .set_member_listening(&owner.room.id, &owner.member.id, &owner.member.id, false)
        .expect_err("不能不听自己");
    assert!(matches!(error, Error::InvalidMessage(_)));

    store
        .set_member_listening(&owner.room.id, &owner.member.id, &member.member.id, false)
        .expect("写入不听名单");
    store
        .leave_room(&owner.room.id, &member.member.id)
        .expect("成员离开");

    let state = store
        .member_listening_state(&owner.room.id, &owner.member.id)
        .expect("读取当前名单");
    assert!(state.not_listening_member_ids.is_empty());
}

#[test]
fn 房间聊天会保存最近消息并裁剪历史() {
    let store = RoomStore::new(8).with_chat_history_limit(2);
    let owner = store.create_room("房主").expect("创建房间");
    let member = store.join_room(&owner.room.id, "队友").expect("加入房间");

    let first = store
        .send_chat_message(&owner.room.id, &owner.member.id, "第一条", Vec::new())
        .expect("发送第一条");
    let second = store
        .send_chat_message(&owner.room.id, &member.member.id, " 第二条 ", Vec::new())
        .expect("发送第二条");
    let third = store
        .send_chat_message(&owner.room.id, &owner.member.id, "第三条", Vec::new())
        .expect("发送第三条");

    assert!(first.id.starts_with("c_"));
    assert_eq!(second.content, "第二条");
    assert_eq!(second.nickname, "队友");
    assert!(third.sent_at_epoch_millis >= second.sent_at_epoch_millis);

    let history = store.chat_history(&owner.room.id).expect("读取历史");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].content, "第二条");
    assert_eq!(history[1].content, "第三条");
}

#[test]
fn 房间聊天拒绝空消息和超长消息() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间");

    let empty = store
        .send_chat_message(&owner.room.id, &owner.member.id, "   ", Vec::new())
        .expect_err("空消息应拒绝");
    assert!(matches!(empty, Error::InvalidMessage(_)));

    let too_long = "a".repeat(501);
    let error = store
        .send_chat_message(&owner.room.id, &owner.member.id, &too_long, Vec::new())
        .expect_err("超长消息应拒绝");
    assert!(matches!(error, Error::InvalidMessage(_)));
}

#[test]
fn 普通成员离开后房间保留() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");
    let member = store
        .join_room(&owner.room.id, "队友")
        .expect("成员加入成功");

    let room = store
        .leave_room(&owner.room.id, &member.member.id)
        .expect("普通成员可以离开");

    assert!(room.members.contains_key(&owner.member.id));
    assert!(!room.members.contains_key(&member.member.id));

    let persisted_room = store
        .get_room(&owner.room.id)
        .expect("普通成员离开后房间仍然存在");

    assert!(persisted_room.members.contains_key(&owner.member.id));
    assert!(!persisted_room.members.contains_key(&member.member.id));
}

#[test]
fn 房主离开后关闭房间() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");

    store
        .leave_room(&owner.room.id, &owner.member.id)
        .expect("房主可以离开并关闭房间");

    let err = store
        .get_room(&owner.room.id)
        .expect_err("房主离开后房间不存在");

    assert!(matches!(err, Error::RoomNotFound));
}

#[test]
fn 成员断线后可以使用恢复凭据回到原身份() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");
    let member = store
        .join_room(&owner.room.id, "队友")
        .expect("成员加入成功");

    let room = store
        .mark_member_disconnected(&owner.room.id, &member.member.id)
        .expect("成员可以被标记断线");
    assert!(!room.members[&member.member.id].connected);

    let resumed = store
        .resume_room(&owner.room.id, &member.member.id, &member.resume_token)
        .expect("恢复凭据有效");

    assert_eq!(resumed.member.id, member.member.id);
    assert_eq!(resumed.member.nickname, "队友");
    assert!(resumed.room.members[&member.member.id].connected);
    assert!(!member.resume_token.is_empty());
}

#[test]
fn 恢复房间需要匹配成员的恢复凭据() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");

    let err = store
        .resume_room(&owner.room.id, &owner.member.id, "wrong-token")
        .expect_err("错误凭据不能恢复房间");

    assert!(matches!(err, Error::InvalidResumeToken));
}

#[test]
fn 普通成员断线超时后被移出房间() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");
    let member = store
        .join_room(&owner.room.id, "队友")
        .expect("成员加入成功");

    store
        .mark_member_disconnected(&owner.room.id, &member.member.id)
        .expect("成员可以被标记断线");
    let expired = store
        .expire_disconnected_member(&owner.room.id, &member.member.id)
        .expect("断线成员超时清理成功")
        .expect("断线成员会被移除");

    assert!(!expired.members.contains_key(&member.member.id));
    assert!(
        store
            .get_room(&owner.room.id)
            .expect("房间仍存在")
            .members
            .contains_key(&owner.member.id)
    );
}

#[test]
fn 房主断线超时后关闭房间() {
    let store = RoomStore::new(8);
    let owner = store.create_room("房主").expect("创建房间成功");

    store
        .mark_member_disconnected(&owner.room.id, &owner.member.id)
        .expect("房主可以被标记断线");
    let expired = store
        .expire_disconnected_member(&owner.room.id, &owner.member.id)
        .expect("断线房主超时清理成功")
        .expect("房主断线超时关闭房间");

    assert_eq!(expired.id, owner.room.id);
    assert!(matches!(
        store.get_room(&owner.room.id),
        Err(Error::RoomNotFound)
    ));
}
