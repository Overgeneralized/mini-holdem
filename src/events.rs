use std::collections::HashMap;

use crate::{cards::{Card, HandRank}, game::{Pot, ShowdownStep}};

#[derive(Debug, Clone)]
pub enum ServerBound {
    Login(String),
    Leave,
    Ready(bool),
    GetPlayerList,
    GameAction(GamePlayerAction)
}

#[derive(Debug, Clone)]
pub enum ClientBound {
    UpdatePlayerList(Vec<(bool, bool, u32, String)>), // is ready, is folded, money, username
    YourId(u8),
    PlayerLeft(String),
    PlayerJoined(String),
    GameStarted([Card; 2]), // private cards
    GameEvent(GameEvent)
}

// the client is able to tell when something is a check, call, bet, raise or an all-in
#[derive(Debug, Clone)]
pub enum GamePlayerAction {
    Check,
    AddMoney(u32), // can be anything: call, bet, raise, all-in
    Fold,
}

#[derive(Debug, Clone)]
pub enum GameEvent {
    PlayerAction(u8, GamePlayerAction),
    OwnedMoneyChange(u8, u32),
    NextPlayer(u8),
    UpdateCurrentBet(u32),
    UpdatePots(Vec<Pot>),
    RevealFlop([Card; 3]),
    RevealTurn(Card),
    RevealRiver(Card),
    Showdown(HashMap<u8, ([Card; 2], HandRank)>),
    ShowdownSteps(Vec<ShowdownStep>)
}
