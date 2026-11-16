# Archived from PLAN.md on 2026-11-16

Closed sections moved verbatim when PLAN.md passed its 100 KB limit. Source: PLAN.md at 4769107 (main): "Follow-ups from task 92" and "Pane-scrollbar defects".

### Follow-ups from task 92 (not for 0.2.0 wave 1)
- [P3] `ensure_collection_id` (`collection.rs:242-257`) has a read-modify-write race: two instances adding an ID at once means the last rename wins. `save_collection_defaults` has the same window.
- [P3] `ensure_private_dir` (`data_dir.rs:92-101`): a trailing slash (e.g. `WISP_DATA_DIR="/x/link/"`) gets past the symlink refusal on macOS. Normalise before the lstat.
- [NOTE] An existing data dir wider than 0700 is not tightened (task 93 may care).

### Pane-scrollbar defects (tasks 46-49, fixed and hand-checked 2026-11-02)
All four defects found hand-checking 43-45 are fixed (see HISTORY). Combined binary 19,014,912 B (+0 B).
**Residuals:** many inherited headers can squeeze the request-header table (the inherited list keeps natural height per spec). (Path, Auth, Settings, Multipart and File sub-tabs now scroll; task 52.) Do not add `.flex()` to the `div().w_full()` wrapper around KvTable virtual rows (46).
**Header limit (47, resolved by 50):** the limit is now 1,000 headers; see the Task 50 residuals below.
