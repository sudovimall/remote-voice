# 后端基础切片 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 让当前 Rust 项目具备可编译的 Axum 服务骨架、房间领域模型、房主发言权限控制和基础 HTTP API。

**Architecture:** 本切片只实现后端基础能力，不接入 WebRTC 媒体转发。HTTP 层负责请求和响应，房间、成员、权限判断放在 `domain::room`，共享状态放在 `AppState`。

**Tech Stack:** Rust 2024、Tokio、Axum、Serde、thiserror、tracing。

---

## 文件结构

- 修改 `src/lib.rs`：导出项目模块和统一 `Result`。
- 修改 `src/error.rs`：定义结构化错误，并实现 Axum 响应转换。
- 修改 `src/config/settings.rs`：增加房间配置默认值。
- 修改 `src/state.rs`：集中保存共享房间状态。
- 修改 `src/app.rs`：构建 Router 并启动服务。
- 创建 `src/transport/mod.rs`：导出传输层模块。
- 修改 `src/transport/http/mod.rs`：实现健康检查和房间 HTTP API。
- 修改 `src/domain/mod.rs`：导出房间领域模块。
- 创建 `src/domain/room.rs`：实现房间、成员、房主权限和内存存储。
- 创建 `tests/room_permissions.rs`：覆盖房主控制成员发言权限。

## Task 1: 房间权限领域模型

**Files:**
- Create: `tests/room_permissions.rs`
- Modify: `src/lib.rs`
- Modify: `src/error.rs`
- Modify: `src/domain/mod.rs`
- Create: `src/domain/room.rs`

- [ ] **Step 1: 写失败测试**

```rust
use voice::domain::room::RoomStore;
use voice::Error;

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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --test room_permissions`

Expected: 编译失败，提示 `voice::domain`、`RoomStore` 或 `Error` 尚未定义。

- [ ] **Step 3: 实现最小领域模型**

实现 `Error`、模块导出、`RoomStore`、`Room`、`Member`、房主权限判断和人数上限。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --test room_permissions`

Expected: 3 个测试通过。

## Task 2: 服务骨架和 HTTP API

**Files:**
- Modify: `src/config/settings.rs`
- Modify: `src/state.rs`
- Modify: `src/app.rs`
- Create: `src/transport/mod.rs`
- Modify: `src/transport/http/mod.rs`

- [ ] **Step 1: 写失败测试**

在 `src/transport/http/mod.rs` 中写单元测试，验证 `POST /api/rooms` 可以创建房间，`GET /api/rooms/:room_id` 可以查询房间。

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test transport::http`

Expected: 编译失败或测试失败，因为 Router 和处理函数尚未实现。

- [ ] **Step 3: 实现最小 HTTP 层**

实现：

- `GET /health`
- `POST /api/rooms`
- `POST /api/rooms/:room_id/join`
- `GET /api/rooms/:room_id`
- `POST /api/rooms/:room_id/members/:member_id/speaking`

HTTP 层只调用 `RoomStore`，不重复实现权限判断。

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test transport::http`

Expected: HTTP 层测试通过。

## Task 3: 全量验证

**Files:**
- No new files.

- [ ] **Step 1: 运行格式化**

Run: `cargo fmt`

Expected: 格式化完成，无错误。

- [ ] **Step 2: 运行所有测试**

Run: `cargo test`

Expected: 所有测试通过。

- [ ] **Step 3: 提交前停止**

不要自动执行 `git commit`。按项目协作限制，先向用户说明本次修改文件和验证结果，等用户明确确认后再提交。
