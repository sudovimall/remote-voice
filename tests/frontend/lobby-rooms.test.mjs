import assert from "node:assert/strict";
import test from "node:test";

import {
  fetchRoomSummaries,
  normalizeRoomSummaries,
} from "../../static/lobby-rooms.mjs";

test("lobby room summaries keep joinable room ids and member counts", () => {
  assert.deepEqual(
    normalizeRoomSummaries([
      { id: " uin90k ", member_count: 3 },
      { id: "", member_count: 1 },
      { id: "abc123", member_count: -1 },
      { id: "room-2", member_count: 0 },
    ]),
    [
      { id: "UIN90K", memberCount: 3 },
      { id: "ROOM-2", memberCount: 0 },
    ],
  );
  assert.deepEqual(normalizeRoomSummaries({}), []);
});

test("lobby room refresh reads summaries from the rooms endpoint", async () => {
  const requests = [];
  const rooms = await fetchRoomSummaries(async (path, options) => {
    requests.push([path, options]);
    return {
      ok: true,
      async json() {
        return [{ id: "ABC123", member_count: 2 }];
      },
    };
  });

  assert.deepEqual(requests, [
    ["/api/rooms", { headers: { accept: "application/json" } }],
  ]);
  assert.deepEqual(rooms, [{ id: "ABC123", memberCount: 2 }]);
  await assert.rejects(
    () => fetchRoomSummaries(async () => ({ ok: false })),
    /房间列表刷新失败/,
  );
});
