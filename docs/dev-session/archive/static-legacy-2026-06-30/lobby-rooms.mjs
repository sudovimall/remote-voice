function normalizedRoomId(value) {
  return typeof value === "string" ? value.trim().toUpperCase() : "";
}

export function normalizeRoomSummaries(rooms) {
  if (!Array.isArray(rooms)) {
    return [];
  }

  return rooms.flatMap((room) => {
    const id = normalizedRoomId(room?.id);
    const memberCount = room?.member_count;
    if (!id || !Number.isInteger(memberCount) || memberCount < 0) {
      return [];
    }

    return [{ id, memberCount }];
  });
}

export async function fetchRoomSummaries(fetchImpl = fetch) {
  const response = await fetchImpl("/api/rooms", {
    headers: {
      accept: "application/json",
    },
  });
  if (!response.ok) {
    throw new Error("房间列表刷新失败。");
  }

  return normalizeRoomSummaries(await response.json());
}
