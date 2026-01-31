use mini_holdem::{cards::HandCategory, events::GamePlayerAction, game::make_game};

fn main() {
    let mut game = make_game(vec![(0, 1000), (1, 1000), (2, 1000)]).unwrap();
    let actions: Vec<GamePlayerAction> = vec![GamePlayerAction::AddMoney(10), GamePlayerAction::AddMoney(5), GamePlayerAction::Check, GamePlayerAction::Check, GamePlayerAction::Check];
    for action in actions {
        println!("advanced game: {:?}", game.advance_game(action).unwrap());
    }
    println!("hello: {}", HandCategory::Flush as u8);
}
