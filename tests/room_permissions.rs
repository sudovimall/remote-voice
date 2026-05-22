use voice::Error;
use voice::domain::room::RoomStore;

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
