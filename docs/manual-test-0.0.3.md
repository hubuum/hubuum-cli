# Manual Test Checklist for hubuum-cli 0.0.3

This document provides an ordered checklist for manually testing the hubuum-cli against an updated hubuum server (0.0.3+). It covers the core functionality and the new/changed surface area from the async rewrite migration, particularly background task support and IAM features.

## Prerequisites

- A running hubuum server with 0.0.3 migration applied
- `hubuum-cli` built with `hubuum_client = { path = "../hubuum_client" }` in `Cargo.toml`
- CLI configured to point at the server (via `config set --key server.hostname --value <your-server>` or environment variables)
- Valid credentials for login

**Note on tests:** The suite's parallel `cargo test` has one known-flaky test. For reliable test runs, use `cargo test -- --test-threads=1`.

## 1. Login / Logout / Identity

**Commands:** `login`, `logout`, `me`, `whoami`, `me groups`, `me tokens`, `me permissions`

### Test Steps

1. **Login**
   ```
   login --username <user> --password <pass>
   ```
   - **Expected:** Login succeeds; token stored; prompt updates with user identity
   - **Prompt status:** Should show logged-in user

2. **Check identity (both forms)**
   ```
   me
   whoami
   ```
   - **Expected:** Both commands return current principal identity (user or service account)

3. **View group memberships**
   ```
   me groups
   ```
   - **Expected:** List of groups the current principal belongs to

4. **View tokens**
   ```
   me tokens
   ```
   - **Expected:** List of active tokens for the current principal

5. **View permissions**
   ```
   me permissions
   ```
   - **Expected:** Effective permissions for the current principal

6. **Logout**
   ```
   logout
   ```
   - **Expected:** Token cleared; prompt updates
   - **Prompt status:** Should show logged-out state

---

## 2. Namespace, Class, Object CRUD

**Commands:** `namespace`, `class`, `object` (create/list/show/modify/delete)

### Test Steps

1. **Create namespace**
   ```
   namespace create --name test-ns --description "Test namespace" --owner <group>
   ```
   - **Expected:** Namespace created and displayed

2. **List namespaces**
   ```
   namespace list
   namespace list --limit 5
   ```
   - **Expected:** Namespaces listed; pagination if applicable

3. **Show namespace**
   ```
   namespace show test-ns
   ```
   - **Expected:** Namespace details displayed

4. **Modify namespace**
   ```
   namespace modify test-ns --description "Updated description"
   ```
   - **Expected:** Namespace updated

5. **Create class**
   ```
   class create --name TestClass --namespace test-ns --description "Test class"
   ```
   - **Expected:** Class created

6. **List classes**
   ```
   class list
   class list --name TestClass
   ```
   - **Expected:** Classes listed

7. **Show class**
   ```
   class show TestClass
   ```
   - **Expected:** Class details displayed; relations section shows related classes (if any)

8. **Modify class**
   ```
   class modify TestClass --description "Updated class description"
   ```
   - **Expected:** Class updated

9. **Create object**
   ```
   object create --name test-obj --class TestClass --namespace test-ns --description "Test object"
   ```
   - **Expected:** Object created

10. **List objects**
    ```
    object list
    object list --class TestClass
    ```
    - **Expected:** Objects listed

11. **Show object**
    ```
    object show --class TestClass test-obj
    ```
    - **Expected:** Object details displayed; relations section shows related objects (if any)

12. **Modify object**
    ```
    object modify --name test-obj --class TestClass --description "Updated object"
    ```
    - **Expected:** Object updated

13. **Delete object**
    ```
    object delete --class TestClass test-obj
    ```
    - **Expected:** Object deleted

14. **Delete class**
    ```
    class delete TestClass
    ```
    - **Expected:** Class deleted

15. **Delete namespace**
    ```
    namespace delete test-ns
    ```
    - **Expected:** Namespace deleted

---

## 3. Relations

**Commands:** `relation class`, `relation object` (create/show/delete/list/direct/graph)

### Test Steps

1. **Create class relation**
   ```
   relation class create --class-a ClassA --class-b ClassB
   ```
   - **Expected:** Class relation created

2. **Show class relation (by pair)**
   ```
   relation class show --class-a ClassA --class-b ClassB
   ```
   - **Expected:** Class relation displayed

3. **Show class relation (by ID)**
   ```
   relation class show --id <relation-id>
   ```
   - **Expected:** Class relation displayed

4. **List related classes**
   ```
   relation class list --root-class ClassA
   relation class list --root-class ClassA --max-depth 3
   ```
   - **Expected:** Related classes listed with depth information

5. **List direct class relations**
   ```
   relation class direct --root-class ClassA
   ```
   - **Expected:** Direct class relations touching ClassA

6. **Show class graph**
   ```
   relation class graph --root-class ClassA
   relation class graph --root-class ClassA --max-depth 3
   ```
   - **Expected:** Class neighborhood graph displayed (classes and relations)

7. **Create object relation**
   ```
   relation object create --class-a ClassA --object-a ObjA --class-b ClassB --object-b ObjB
   ```
   - **Expected:** Object relation created

8. **Show object relation (by pair)**
   ```
   relation object show --class-a ClassA --object-a ObjA --class-b ClassB --object-b ObjB
   ```
   - **Expected:** Object relation displayed

9. **Show object relation (by ID)**
   ```
   relation object show --id <relation-id>
   ```
   - **Expected:** Object relation displayed

10. **List related objects**
    ```
    relation object list --root-class ClassA --root-object ObjA
    relation object list --root-class ClassA --root-object ObjA --max-depth 2 --include-self-class
    ```
    - **Expected:** Related objects listed

11. **List direct object relations**
    ```
    relation object direct --root-class ClassA --root-object ObjA
    ```
    - **Expected:** Direct object relations touching ObjA

12. **Show object graph**
    ```
    relation object graph --root-class ClassA --root-object ObjA
    relation object graph --root-class ClassA --root-object ObjA --max-depth 2
    ```
    - **Expected:** Object neighborhood graph displayed (objects and relations)

13. **Delete object relation (by pair)**
    ```
    relation object delete --class-a ClassA --object-a ObjA --class-b ClassB --object-b ObjB
    ```
    - **Expected:** Object relation deleted

14. **Delete class relation (by pair)**
    ```
    relation class delete --class-a ClassA --class-b ClassB
    ```
    - **Expected:** Class relation deleted

---

## 4. Search

**Commands:** `search` (with `--stream`, `--kind`, `--search-class-schema`, `--search-object-data`)

### Test Steps

1. **Basic search**
   ```
   search "test"
   search --query "test"
   ```
   - **Expected:** Results across namespaces, classes, and objects

2. **Search with kind filter**
   ```
   search "test" --kind class --kind object
   ```
   - **Expected:** Only classes and objects returned

3. **Search class schema**
   ```
   search "property" --kind class --search-class-schema
   ```
   - **Expected:** Classes with schema matching "property"

4. **Search object data**
   ```
   search "value" --kind object --search-object-data
   ```
   - **Expected:** Objects with JSON data containing "value"

5. **Streaming search**
   ```
   search "test" --stream --kind class --kind object
   ```
   - **Expected:** Server-sent events displayed; batch-by-batch results

6. **Paginated search**
   ```
   search "common" --limit-per-kind 2
   ```
   - **Expected:** Max 2 results per kind; cursor command for next page
   - **Prompt status (if enabled):** "Press Enter for next page" or "Type 'next' for next page"

---

## 5. Reports

**Commands:** `report run`, `report list/show/create/modify/delete`, `jobs`, `task`

### Test Steps (Background Tasks)

1. **Run report (background default)**
   ```
   report run --scope namespace --template <template-name>
   ```
   - **Expected:** Task submitted; job ID shown; prompt updates with job status
   - **Prompt status:** Should show running job count (e.g., "[jobs: 1]")

2. **List background jobs**
   ```
   jobs list
   bg list
   ```
   - **Expected:** Local background jobs listed with state (running/completed)

3. **Show job details**
   ```
   jobs show <local-id>
   ```
   - **Expected:** Job details including task ID and state

4. **View job output (once completed)**
   ```
   jobs output <local-id>
   ```
   - **Expected:** Report output displayed
   - **Prompt status:** Job removed from running count

5. **View task details**
   ```
   task show <task-id>
   ```
   - **Expected:** Server task details (status, timestamps)

6. **View task output**
   ```
   task output <task-id>
   ```
   - **Expected:** Task output displayed

7. **Watch existing task**
   ```
   jobs watch <task-id>
   ```
   - **Expected:** Task tracked as local background job

8. **Forget job**
   ```
   jobs forget <local-id>
   ```
   - **Expected:** Job removed from local tracking

### Test Steps (Report Templates & Options)

9. **Run report with --wait (foreground)**
   ```
   report run --scope namespace --template <template-name> --wait
   ```
   - **Expected:** CLI blocks until task completes; output displayed

10. **Run report with --wait --timeout**
    ```
    report run --scope namespace --template <template-name> --wait --timeout 30
    ```
    - **Expected:** CLI blocks max 30 seconds; timeout error if not done

11. **Run report with relation options**
    ```
    report run --scope class --class <class-name> --relation-depth 3
    report run --scope object --class <class-name> --object <object-name> --include-related "key:ClassID:2"
    ```
    - **Expected:** Report includes related entities per specified depth/filters

12. **Create report template**
    ```
    report create --name test-report --namespace test-ns --description "Test report" --content-type "text/plain" --template "Hello {{ scope }}"
    ```
    - **Expected:** Report template created

13. **List report templates**
    ```
    report list
    ```
    - **Expected:** Report templates listed

14. **Show report template**
    ```
    report show test-report
    ```
    - **Expected:** Template details displayed

15. **Modify report template**
    ```
    report modify test-report --description "Updated report template"
    ```
    - **Expected:** Template updated

16. **Delete report template**
    ```
    report delete test-report
    ```
    - **Expected:** Template deleted

---

## 6. Imports

**Commands:** `import submit`, `import show`, `import results`

### Test Steps

1. **Submit import (background default)**
   ```
   import submit --file import.json
   ```
   - **Expected:** Import task submitted; job ID shown; prompt updates with job status
   - **Prompt status:** Should show running job

2. **Submit import with idempotency key**
   ```
   import submit --file import.json --idempotency-key "unique-key-123"
   ```
   - **Expected:** Import submitted with idempotency key

3. **Submit import with --wait**
   ```
   import submit --file import.json --wait
   ```
   - **Expected:** CLI blocks until import completes

4. **Show import task**
   ```
   import show <task-id>
   ```
   - **Expected:** Import task details displayed

5. **List import results**
   ```
   import results <task-id>
   ```
   - **Expected:** Import results listed (created/updated/failed entities)

6. **View import via jobs output**
   ```
   jobs output <local-id>
   ```
   - **Expected:** Import output displayed

---

## 7. Tasks

**Commands:** `task list`, `task show`, `task events`, `task output`, `task queue`

### Test Steps

1. **List all tasks**
   ```
   task list
   ```
   - **Expected:** All tasks listed

2. **List tasks by kind**
   ```
   task list --kind import
   task list --kind report
   task list --kind remote_call
   ```
   - **Expected:** Only tasks of specified kind

3. **List tasks by status**
   ```
   task list --status pending
   task list --status running
   task list --status completed
   ```
   - **Expected:** Only tasks with specified status

4. **Show task details**
   ```
   task show <task-id>
   ```
   - **Expected:** Task details displayed

5. **List task events**
   ```
   task events <task-id>
   ```
   - **Expected:** Task event history displayed (transitions, errors)

6. **Show task queue state**
   ```
   task queue
   ```
   - **Expected:** Queue statistics (pending/running/completed counts)

---

## 8. Service Accounts

**Commands:** `service-account` (create/list/show/delete/disable), `service-account token` (create/list/revoke)

### Test Steps

1. **Create service account**
   ```
   service-account create --name test-sa --description "Test SA" --owner-group-id <group-id>
   ```
   - **Expected:** Service account created

2. **List service accounts**
   ```
   service-account list
   ```
   - **Expected:** Service accounts listed

3. **Show service account**
   ```
   service-account show test-sa
   ```
   - **Expected:** Service account details displayed

4. **Disable service account**
   ```
   service-account disable test-sa
   ```
   - **Expected:** Service account disabled

5. **Create service account token**
   ```
   service-account token create test-sa --token-name "api-token" --description "API access"
   service-account token create test-sa --token-name "expiring-token" --expires-at "2027-12-31T23:59:59Z"
   ```
   - **Expected:** Token created; raw token displayed ONCE with warning
   - **Note:** Capture the token; it won't be shown again

6. **List service account tokens**
   ```
   service-account token list test-sa
   ```
   - **Expected:** Tokens listed (no raw token values shown)

7. **Revoke service account token**
   ```
   service-account token revoke test-sa --token-id <token-id>
   ```
   - **Expected:** Token revoked

8. **Delete service account**
   ```
   service-account delete test-sa
   ```
   - **Expected:** Service account deleted

---

## 9. User Management

**Commands:** `user` (create/list/show/modify/delete/set-password), `user token` (create/list/revoke)

### Test Steps

1. **Create user**
   ```
   user create --username testuser --email testuser@example.com
   ```
   - **Expected:** User created; random password displayed

2. **List users**
   ```
   user list
   user list --username testuser
   ```
   - **Expected:** Users listed

3. **Show user**
   ```
   user show testuser
   ```
   - **Expected:** User details displayed

4. **Modify user**
   ```
   user modify testuser --email newemail@example.com
   ```
   - **Expected:** User updated

5. **Modify user with rename (note limitation)**
   ```
   user modify testuser --rename newuser
   ```
   - **Expected:** Command may error if server doesn't support user rename (known limitation in 0.0.3 server)

6. **Set user password**
   ```
   user set-password testuser --password "newpassword"
   ```
   - **Expected:** Password updated

7. **Create user token**
   ```
   user token create testuser --name "cli-token" --description "CLI access"
   user token create testuser --name "expiring-token" --expires-at "2027-12-31T23:59:59Z"
   ```
   - **Expected:** Token created; raw token displayed ONCE with warning

8. **List user tokens**
   ```
   user token list testuser
   ```
   - **Expected:** Tokens listed

9. **Revoke user token**
   ```
   user token revoke testuser --token-id <token-id>
   ```
   - **Expected:** Token revoked

10. **Delete user**
    ```
    user delete testuser
    ```
    - **Expected:** User deleted

---

## 10. Permissions

**Commands:** `namespace permissions`, `namespace principal-permissions`, `me permissions`

### Test Steps

1. **Grant namespace permissions to group**
   ```
   namespace permissions set test-ns --group editors --all
   namespace permissions set test-ns --group readers --ReadCollection --ReadClass --ReadObject
   ```
   - **Expected:** Permissions granted to group

2. **List namespace permissions**
   ```
   namespace permissions list test-ns
   ```
   - **Expected:** All permissions for the namespace displayed (by group)

3. **Show principal permissions for namespace**
   ```
   namespace principal-permissions test-ns --principal-id <principal-id>
   ```
   - **Expected:** Effective permissions for the specified principal in the namespace

4. **View current principal's effective permissions**
   ```
   me permissions
   ```
   - **Expected:** All effective permissions for current principal across all namespaces

---

## 11. Remote Targets

**Commands:** `remote-target` (create/list/show/update/delete/invoke)

### Test Steps

1. **Create remote target**
   ```
   remote-target create --name test-target --namespace-id <ns-id> --description "Test target" \
     --method post --url "https://example.com/webhook" \
     --subject-types "object" \
     --auth-type bearer --auth-secret "secret-token"
   ```
   - **Expected:** Remote target created

2. **List remote targets**
   ```
   remote-target list
   ```
   - **Expected:** Remote targets listed

3. **Show remote target**
   ```
   remote-target show test-target
   ```
   - **Expected:** Remote target details displayed
   - **Note:** Auth secrets should be redacted (not shown in plain text)

4. **Update remote target**
   ```
   remote-target update test-target --description "Updated target"
   ```
   - **Expected:** Remote target updated

5. **Invoke remote target (background)**
   ```
   remote-target invoke test-target --subject namespace --namespace-id <ns-id>
   ```
   - **Expected:** Remote call task submitted; job ID shown; prompt updates with job status
   - **Prompt status:** Should show running job

6. **View remote call task**
   ```
   task show <task-id>
   task events <task-id>
   ```
   - **Expected:** Task details and events displayed
   - **Note:** In 0.0.3, remote-call task results are NOT fetchable via `jobs output` or `task output` (known limitation)

7. **Invoke remote target with --wait**
   ```
   remote-target invoke test-target --subject namespace --namespace-id <ns-id> --wait
   ```
   - **Expected:** CLI blocks until remote call completes

8. **Delete remote target**
   ```
   remote-target delete test-target
   ```
   - **Expected:** Remote target deleted

---

## 12. Groups

**Commands:** `group` (create/list/show/modify), `group add_user`, `group remove_user`

### Test Steps

1. **Create group**
   ```
   group create --groupname testgroup --description "Test group"
   ```
   - **Expected:** Group created

2. **List groups**
   ```
   group list
   ```
   - **Expected:** Groups listed

3. **Show group**
   ```
   group show testgroup
   ```
   - **Expected:** Group details and members displayed

4. **Add user to group**
   ```
   group add_user --groupname testgroup --username <user>
   ```
   - **Expected:** User added to group

5. **Remove user from group**
   ```
   group remove_user --groupname testgroup --username <user>
   ```
   - **Expected:** User removed from group

6. **Modify group**
   ```
   group modify testgroup --description "Updated group description"
   ```
   - **Expected:** Group updated

---

## 13. Config

**Commands:** `config show`, `config paths`, `config set`, `config unset`

### Test Steps

1. **Show all configuration**
   ```
   config show
   ```
   - **Expected:** All config keys/values with sources displayed

2. **Show specific config key**
   ```
   config show --key server.hostname
   ```
   - **Expected:** Single config entry displayed with source

3. **Show config file paths**
   ```
   config paths
   ```
   - **Expected:** System/user/custom/write paths displayed

4. **Set config value**
   ```
   config set --key repl.enter_fetches_next_page --value true
   ```
   - **Expected:** Value persisted; session reloaded

5. **Unset config value**
   ```
   config unset --key repl.enter_fetches_next_page
   ```
   - **Expected:** Value removed; session reloaded

---

## 14. Miscellaneous

**Commands:** `help`, `login`/`logout`

### Test Steps

1. **Show help**
   ```
   help
   ```
   - **Expected:** Help information displayed

2. **Show command tree**
   ```
   help --tree
   ```
   - **Expected:** Full command tree displayed

---

## Known Limitations in 0.0.3

- **User rename:** The server may not support renaming users (will error if attempted)
- **Remote-call task output:** Remote call task results cannot be fetched via `jobs output` or `task output` in 0.0.3; use `task show` and `task events` to observe completion

---

## Summary

This checklist covers:
- **Authentication & Identity:** Login, logout, me/whoami, me groups/tokens/permissions
- **Core CRUD:** Namespace, class, object (create/list/show/modify/delete)
- **Relations:** Class and object relations (create/show/delete/list/direct/graph)
- **Search:** Plain search, streaming, kind filters, class schema, object data
- **Reports:** Run (background/foreground), templates CRUD, relation-depth, include-related
- **Imports:** Submit (background/foreground), show, results
- **Tasks:** List (kind/status filters), show, events, output, queue
- **IAM:** Service accounts (CRUD, disable, tokens), users (CRUD, set-password, tokens), permissions (namespace group/principal, me)
- **Remote Targets:** CRUD, invoke (background), auth secret redaction
- **Groups:** CRUD, add/remove users
- **Config:** Show, paths, set, unset

All commands cross-checked against source code (`src/commands/*.rs`). Background task support (reports, imports, remote-target invoke) validated with job tracking (`jobs`/`bg` alias, `task`), prompt status updates, and output retrieval.
