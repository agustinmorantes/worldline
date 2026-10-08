// @generated automatically by Diesel CLI.

diesel::table! {
    games (id) {
        id -> Integer,
        uuid -> Text,
        title -> Text,
        title_normalized -> Text,
        folder_slug -> Text,
        active_slot_id -> Nullable<Integer>,
        icon_path -> Nullable<Text>,
        accent_hue -> Integer,
        accent_hue_source -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    scan_roots (id) {
        id -> Integer,
        path -> Text,
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
        uuid -> Text,
        game_id -> Integer,
        folder_slug -> Text,
        path -> Text,
        label -> Nullable<Text>,
        exclusions -> Text,
        retired_at -> Nullable<Timestamp>,
        created_at -> Timestamp,
        updated_at -> Timestamp,
    }
}

diesel::table! {
    versions (id) {
        id -> Integer,
        uuid -> Text,
        slot_id -> Integer,
        parent_id -> Nullable<Integer>,
        version_number -> Integer,
        folder_slug -> Text,
        reason -> Text,
        tag -> Nullable<Text>,
        retired -> Bool,
        created_at -> Timestamp,
        origin_device -> Nullable<Text>,
    }
}

diesel::joinable!(slots -> games (game_id));
diesel::joinable!(tracked_dirs -> games (game_id));
diesel::joinable!(versions -> slots (slot_id));

diesel::allow_tables_to_appear_in_same_query!(games, scan_roots, slots, tracked_dirs, versions,);
