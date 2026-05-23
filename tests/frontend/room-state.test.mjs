import assert from "node:assert/strict";
import test from "node:test";

import {
  createRoomSignal,
  joinRoomSignal,
  membersForRoom,
  nextRoomSnapshot,
  resumeRoomSignal,
  startScreenShareSignal,
  stopScreenShareSignal,
  websocketUrl,
} from "../../static/room-state.mjs";

test("room signals create an owner and join a target room", () => {
  assert.deepEqual(createRoomSignal("房主", "create-1"), {
    type: "create_room",
    request_id: "create-1",
    nickname: "房主",
  });
  assert.deepEqual(
    joinRoomSignal(
      {
        roomId: "ABC123",
        nickname: "队友",
      },
      "join-1",
    ),
    {
      type: "join_room",
      request_id: "join-1",
      room_id: "ABC123",
      nickname: "队友",
    },
  );
});

test("room resume signal carries tab session credentials", () => {
  assert.deepEqual(
    resumeRoomSignal(
      {
        roomId: "ABC123",
        memberId: "m_owner",
        resumeToken: "resume-secret",
      },
      "resume-1",
    ),
    {
      type: "resume_room",
      request_id: "resume-1",
      room_id: "ABC123",
      member_id: "m_owner",
      resume_token: "resume-secret",
    },
  );
});

test("screen share signals start and stop sharing", () => {
  assert.deepEqual(startScreenShareSignal("screen-start"), {
    type: "start_screen_share",
    request_id: "screen-start",
  });
  assert.deepEqual(stopScreenShareSignal("screen-stop"), {
    type: "stop_screen_share",
    request_id: "screen-stop",
  });
});

test("websocket url tracks the current page protocol", () => {
  assert.equal(
    websocketUrl({ href: "http://127.0.0.1:8080/rooms/ABC123", protocol: "http:" }),
    "ws://127.0.0.1:8080/ws",
  );
  assert.equal(
    websocketUrl({ href: "https://voice.example/rooms/ABC123", protocol: "https:" }),
    "wss://voice.example/ws",
  );
});

test("members sort owner first and then by nickname", () => {
  const members = membersForRoom({
    owner_member_id: "m_owner",
    members: {
      m_z: { id: "m_z", nickname: "周末", role: "member" },
      m_owner: { id: "m_owner", nickname: "房主", role: "owner" },
      m_a: { id: "m_a", nickname: "阿木", role: "member" },
    },
  });

  assert.deepEqual(
    members.map((member) => member.id),
    ["m_owner", "m_a", "m_z"],
  );
});

test("room snapshots follow room messages and room closure", () => {
  const currentRoom = { id: "ABC123", members: {} };
  const joinedRoom = { id: "ABC123", members: { m_owner: { id: "m_owner" } } };

  assert.equal(
    nextRoomSnapshot(currentRoom, {
      type: "joined_room",
      room: joinedRoom,
    }),
    joinedRoom,
  );
  assert.equal(
    nextRoomSnapshot(joinedRoom, {
      type: "renegotiation_needed",
      member_id: "m_owner",
    }),
    joinedRoom,
  );
  assert.deepEqual(
    nextRoomSnapshot(joinedRoom, {
      type: "screen_share_started",
      member_id: "m_member",
      nickname: "队友",
    }).screen_share,
    {
      member_id: "m_member",
      nickname: "队友",
    },
  );
  assert.equal(
    nextRoomSnapshot(
      {
        ...joinedRoom,
        screen_share: { member_id: "m_member", nickname: "队友" },
      },
      {
        type: "screen_share_stopped",
        member_id: "m_member",
      },
    ).screen_share,
    null,
  );
  assert.equal(
    nextRoomSnapshot(joinedRoom, {
      type: "room_closed",
      room_id: "ABC123",
    }),
    null,
  );
});
