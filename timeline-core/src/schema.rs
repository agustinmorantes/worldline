// @generated automatically by Diesel CLI.

diesel::table! {
    games (id) {
        id -> Integer,
        title -> Text,
        active_version_id -> Nullable<Integer>,
        icon_path -> Nullable<Text>,
    }
}

diesel::table! {
    tracked_dirs (id) {
        id -> Integer,
        path -> Text,
        game_id -> Integer,
    }
}

diesel::table! {
    versions (id) {
        id -> Integer,
        version_number -> Integer,
        game_id -> Integer,
        tag -> Nullable<Text>,
        created_at -> Timestamp,
        parent_id -> Nullable<Integer>,
    }
}

diesel::table! {
    scan_roots (id) {
        id -> Integer,
        path -> Text,
    }
}

diesel::joinable!(tracked_dirs -> games (game_id));
diesel::joinable!(versions -> games (game_id));

diesel::allow_tables_to_appear_in_same_query!(games, scan_roots, tracked_dirs, versions,);
