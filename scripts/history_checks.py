"""History regressions run inside the pinned, disposable integration stack."""

from datetime import datetime, timedelta, timezone
import time


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00"))


def verify_history(api, cli, token, collection):
    cls = api("POST", "/api/v1/classes", {
        "name": collection["name"] + "-history", "description": "history-1",
        "collection_id": collection["id"], "json_schema": None, "validate_schema": False,
    }, token)
    class_path = f'/api/v1/classes/{cls["id"]}'
    obj = api("POST", class_path + "/", {
        "name": "history", "description": "history fixture",
        "collection_id": collection["id"], "hubuum_class_id": cls["id"],
        "data": {"value": 1, "nullable": None},
    }, token)
    object_path = f'{class_path}/{obj["id"]}'
    # Exceed both the timeline and snapshot batch size (250). Every revision
    # has a distinct payload so dropped, duplicated, or mismatched reads fail.
    for revision in range(2, 254):
        api("PATCH", object_path, {"data": {"value": revision, "nullable": None}}, token)
    for revision in range(2, 4):
        api("PATCH", class_path, {"description": f"history-{revision}"}, token)

    class_scope = ["--class-id", str(cls["id"])]
    object_scope = class_scope + ["--object-id", str(obj["id"])]
    for kind, scope, count in [("class", class_scope, 3), ("object", object_scope, 253)]:
        snapshots = cli("history", kind, *scope, "--limit", "250", "--all")
        snapshots.sort(key=lambda row: row["history_id"])
        assert [row["revision"] for row in snapshots] == list(range(1, count + 1))
        for row in snapshots:
            if kind == "object":
                assert row["data"] == {"value": row["revision"], "nullable": None}, row
            else:
                assert row["description"] == f'history-{row["revision"]}', row
        first, middle, latest = snapshots[0], snapshots[1], snapshots[-1]

        # Derive calendar windows from server timestamps, including when the
        # fixture spans a second, day, month, or year boundary.
        instant = timestamp(middle["valid_from"]).astimezone(timezone.utc)
        for precision in [4, 7, 10, 13, 16, 19]:
            prefix = instant.isoformat()[:precision]
            expected = {row["history_id"]: row for row in snapshots
                        if timestamp(row["valid_from"]).astimezone(timezone.utc).isoformat().startswith(prefix)}
            for command in [kind, "show"]:
                rows = cli("history", command, *scope, "--at", prefix)
                assert len(rows) == len(expected), (kind, command, prefix, len(rows), len(expected))
                assert {row["history_id"]: row for row in rows} == expected
            page = cli("history", kind, *scope, "--at", prefix, "--include-total")
            assert page["returned_count"] == page["total_count"] == len(expected), page
            assert page["next_cursor"] is None
        assert cli("history", "show", *scope, "--at", "2000") == []
        assert cli("history", "show", *scope, "--at", middle["valid_from"]) == middle
        offset = instant.astimezone(timezone(timedelta(hours=2))).isoformat()
        assert cli("history", "show", *scope, "--at", offset) == middle

        # Bracket the CLI clock sample. The returned validity interval must
        # overlap the requested instant range; no fixed wall-clock fixture or
        # assumption about subprocess duration is needed.
        age = (datetime.now(timezone.utc) - timestamp(first["valid_from"])).total_seconds()
        time.sleep(max(0, 1.1 - age))
        for relative, seconds in [("now", 0), ("0s", 0), ("1s", 1), ("-1s", 1), ("0mo0w0d0h0m1s", 1)]:
            before = datetime.now(timezone.utc) - timedelta(seconds=seconds)
            row = cli("history", "show", *scope, "--at", relative)
            after = datetime.now(timezone.utc) - timedelta(seconds=seconds)
            assert row in snapshots, (relative, row)
            assert timestamp(row["valid_from"]) <= after, (relative, row, before, after)
            assert row["valid_to"] is None or timestamp(row["valid_to"]) > before, (relative, row, before, after)
            if seconds == 0:
                assert row == latest

        timeline = ["history", "timeline", kind, *scope]
        field = "/data/value" if kind == "object" else "/description"

        def summary(before, after):
            if kind == "object":
                return f'{field}: {before["revision"]} → {after["revision"]}'
            return f'{field}: "history-{before["revision"]}" → "history-{after["revision"]}"'

        # A one-entry window excludes both comparison references. Test both
        # navigation directions and pipeline preservation of the full envelope.
        window = ["--from", middle["valid_from"], "--until", snapshots[2]["valid_from"]]
        for direction in ["older", "newer"]:
            for flags, mode, reference, expected in [
                ([], "previous", first, summary(first, middle)),
                (["--against-latest"], "latest", latest, summary(middle, latest)),
            ]:
                page = cli(*timeline, *window, "--direction", direction, "--limit", "1",
                           "--include-total", *flags, "|", "JQ", ".")
                assert page["returned_count"] == page["total_count"] == 1, page
                assert page["page_limit"] == 1 and page["next_cursor"] is None, page
                entry = page["items"][0]
                assert entry["history_id"] == middle["history_id"], entry
                assert entry["changes"] == {
                    "mode": mode, "against_history_id": reference["history_id"], "summary": expected,
                }, entry

        # Follow actual server cursors in both modes. Neither the displayed
        # page nor the cursor may replace the comparison baseline.
        for flags, mode in [([], "previous"), (["--against-latest"], "latest")]:
            page = cli(*timeline, "--limit", "1", *flags)
            assert page["next_cursor"] and page["items"][0]["history_id"] == latest["history_id"], page
            if flags:
                assert page["items"][0]["changes"]["summary"] == "Latest retained entry"
            following = cli(*timeline, "--limit", "1", "--cursor", page["next_cursor"], *flags)
            entry = following["items"][0]
            assert entry["history_id"] == snapshots[-2]["history_id"], entry
            reference = latest if flags else snapshots[-3]
            expected = summary(snapshots[-2], latest) if flags else summary(reference, snapshots[-2])
            assert entry["changes"] == {
                "mode": mode, "against_history_id": reference["history_id"], "summary": expected,
            }, entry
        complete = cli(*timeline, "--limit", "1", "--all")
        assert complete["returned_count"] == count and complete["next_cursor"] is None, complete
        assert {row["history_id"] for row in complete["items"]} == {row["history_id"] for row in snapshots}
        created = next(row for row in complete["items"] if row["history_id"] == first["history_id"])
        assert created["changes"]["against_history_id"] is None
        assert created["changes"]["summary"].startswith("Created\n"), created
        text = cli(*timeline, *window, "--against-latest", output="text")
        assert "changes" in text and field in text and "→" in text, text

    # Deleted snapshots remain available by ID and in shortened-date windows.
    api("DELETE", object_path, token=token)
    deleted_page = cli("history", "timeline", "object", *object_scope, "--limit", "1")
    deleted = deleted_page["items"][0]
    assert deleted["changes"]["summary"] == "Deleted", deleted
    date = timestamp(deleted["valid_from"]).astimezone(timezone.utc).date().isoformat()
    rows = cli("history", "show", *object_scope, "--at", date)
    snapshot = next(row for row in rows if row["history_id"] == deleted["history_id"])
    assert snapshot["op"] == "D" and snapshot["data"]["nullable"] is None, snapshot
    older = cli("history", "timeline", "object", *object_scope, "--anchor", str(deleted["history_id"]),
                "--limit", "1", "--against-latest")
    assert older["items"][0]["changes"] == {
        "mode": "latest", "against_history_id": deleted["history_id"],
        "summary": "Deleted in latest retained entry",
    }, older
    print("PASS: history date prefixes, relative instants, paged snapshots, comparison boundaries, "
          "latest/deleted references, and timeline pipelines", flush=True)
