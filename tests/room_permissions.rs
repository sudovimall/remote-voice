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
