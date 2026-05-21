// @generated automatically by Diesel CLI.

diesel::table! {
    games (id) {
        id -> Integer,
        uuid -> Nullable<Text>,
        title -> Text,
        title_normalized -> Nullable<Text>,
        folder_slug -> Nullable<Text>,
        active_version_id -> Nullable<Integer>,
        active_slot_id -> Nullable<Integer>,
        icon_path -> Nullable<Text>,
    }
}

diesel::table! {
    slots (id) {
        id -> Integer,
        uuid -> Text,
        game_id -> Integer,
        name -> Text,
        name_normalized -> Text,
        folder_slug -> Text,
        active_version_id -> Nullable<Integer>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    tracked_dirs (id) {
        id -> Integer,
        uuid -> Nullable<Text>,
        folder_slug -> Nullable<Text>,
        path -> Text,
        game_id -> Integer,
    }
}

diesel::table! {
    versions (id) {
        id -> Integer,
        uuid -> Nullable<Text>,
        folder_slug -> Nullable<Text>,
        version_number -> Integer,
        game_id -> Integer,
        slot_id -> Nullable<Integer>,
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
diesel::joinable!(slots -> games (game_id));
diesel::joinable!(versions -> games (game_id));
diesel::joinable!(versions -> slots (slot_id));

diesel::allow_tables_to_appear_in_same_query!(games, scan_roots, slots, tracked_dirs, versions,);
