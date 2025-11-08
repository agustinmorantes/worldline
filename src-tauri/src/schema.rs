// @generated automatically by Diesel CLI.

diesel::table! {
    games (id) {
        id -> Integer,
        title -> Text,
    }
}

diesel::table! {
    tracked_dirs (id) {
        id -> Integer,
        path -> Text,
        game_id -> Integer,
    }
}

diesel::joinable!(tracked_dirs -> games (game_id));

diesel::allow_tables_to_appear_in_same_query!(games, tracked_dirs,);
