"""CLI discovery and reviewed suggestion lifecycle against the owned test server."""


def verify_discovery(api, cli, eventually, token, collection_id, prefix):
    name = prefix + "_suggestions"
    created = api("POST", "/api/v1/classes", {
        "name": name, "description": "Discovery and suggestion contract",
        "collection_id": collection_id, "json_schema": None, "validate_schema": False,
    }, token)
    class_id = created["id"]
    path = f"/api/v1/classes/{class_id}"
    obj = None
    try:
        obj = api("POST", path + "/", {
            "name": prefix + "_sample", "description": "Structured fields",
            "collection_id": collection_id, "hubuum_class_id": class_id,
            "data": {"hostname": "host", "a.b": True, "a": {"b": 7},
                     "[#]": "literal", "values": [1, 2], "nullable": None},
        }, token)
        rows = cli("class", "fields", "--name", name, "--limit", "100")
        literal = next(row for row in rows if row["Path"] == [{"kind": "property", "name": "a.b"}])
        nested = next(row for row in rows if row["Path"] == [{"kind": "property", "name": "a"}, {"kind": "property", "name": "b"}])
        assert literal["TypeCounts"] == {"bool": 1}
        assert nested["TypeCounts"] == {"number": 1}
        array = next(row for row in rows if row["Path"] == [{"kind": "property", "name": "values"}, {"kind": "array_item"}])
        assert array["Count"] == 2 and array["Objects"] == 1

        def schema(command, *options):
            return cli("class", "schema", command, "--class", name, *options)

        def task_succeeded(task_id):
            task = api("GET", f"/api/v1/tasks/{task_id}", token=token)
            assert task["status"] == "succeeded", task["status"]
            return task

        for preset in ["strict", "balanced", "expansive"]:
            active = schema("show")["active"]["revision"]
            suggestion = schema("suggest", "--preset", preset, "--limit", "100", "--wait", "--timeout", "60")
            assert suggestion["status"] == "candidate" and suggestion["sample_valid"] is True
            assert suggestion["options"]["preset"] == preset
            task_id = suggestion["task_id"]
            assert schema("suggestion", "--task", str(task_id)) == suggestion
            assert schema("show")["active"]["revision"] == active
            revision = schema("stage", "--from-suggestion", str(task_id), "--validate", "true")["revision"]
            impact = schema("impact", "--revision", str(revision))["task_id"]

            def compatible():
                work = schema("work", "--task", str(impact))
                assert work["status"] == "complete" and work["readiness"] == "compatible"
                return work

            eventually(compatible)
            activation = schema("activate", "--revision", str(revision), "--expected-active-revision", str(active), "--impact-task", str(impact))
            assert activation["active"]["revision"] == revision
            if activation.get("task_id"):
                eventually(lambda: task_succeeded(activation["task_id"]))
        print("PASS: field inventory, structured paths, three suggestion presets, review, staging, whole-class impact, and explicit activation", flush=True)
    finally:
        if obj:
            api("DELETE", f"{path}/{obj['id']}", token=token)
        api("DELETE", path, token=token)
