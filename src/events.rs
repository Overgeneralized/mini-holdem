use crate::{cards::{Card, HandRank}, game::{Pot, ShowdownStep}};

pub type ShowdownInfo = (Vec<([Card; 2], [Card; 5], HandRank)>, Vec<ShowdownStep>);

#[derive(Debug, Clone)]
pub enum ServerBound {
    Login(String),
    Disconnect,
    Ready(bool),
    GetPlayerList,
    GameAction(GamePlayerAction)
}

#[derive(Debug, Clone)]
pub enum ClientBound {
    UpdatePlayerList(Vec<(PlayerState, u32, String)>), // state, money, username
    YourIndex(u8),
    PlayerLeft(String),
    PlayerJoined(String),
    GameStarted([Card; 2]), // player id and private cards
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
    Showdown(ShowdownInfo),
    InGamePlayerLeave(u8)
}

#[derive(Debug, Clone)]
pub enum PlayerState {
    NotReady,
    Ready,
    InGame,
    Folded,
    Left
}
impl PlayerState {
    pub fn from_byte(byte: u8) -> Option<Self> {
        Some(match byte {
            0 => Self::NotReady,
            1 => Self::Ready,
            2 => Self::InGame,
            3 => Self::Folded,
            4 => Self::Left,
            _ => return None
        })
    }
}
