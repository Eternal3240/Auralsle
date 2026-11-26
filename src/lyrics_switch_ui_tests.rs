//! 换歌与歌曲信息补全时的歌词状态：不串歌、不闪烁、不叠加网络线程。
use super::*;
use crate::smtc::NowPlaying;

fn song(source:&str,title:&str,artist:&str,duration:f64)->NowPlaying{
    // 播放器上报为音乐：歌手暂时为空时仍按歌曲处理（与真实客户端一致）。
    NowPlaying{source:source.into(),title:title.into(),artist:artist.into(),duration,playing:true,position:7.0,
        playback_type:crate::smtc::MediaKind::Music,..Default::default()}
}

fn lyrics_on(h:&mut H){h.app.opts.fetch_lyrics=true;h.app.settings.island_lyrics=true;h.app.show_lyrics=true;}

/// 用一个阻塞中的假线程占住网络名额：排队的查询不会真的联网。
fn busy(h:&mut H)->(std::sync::mpsc::Sender<()>,std::thread::ThreadId){
    let (release,wait)=std::sync::mpsc::channel::<()>();
    let worker=std::thread::spawn(move||{let _=wait.recv();});
    let id=worker.thread().id();h.app.ly_worker=Some(worker);(release,id)
}

fn finish(h:&mut H,release:std::sync::mpsc::Sender<()>){
    release.send(()).unwrap();h.app.ly_worker.take().unwrap().join().unwrap();
}

/// 模拟当前歌曲已查到同步歌词。
fn showing(h:&mut H,media:&NowPlaying,lrc:&str){
    h.media=media.clone();h.frame(vec![]);
    h.app.ly=crate::app::LyricsState{
        key:format!("{}|{}|{:.0}",media.title,media.artist,media.duration),
        song:Some(crate::app::LyricSong{source:media.source.clone(),title:media.title.clone(),artist:media.artist.clone(),duration:media.duration,song_id:media.song_id}),
        lyrics:Some(crate::lyrics::parse_lrc(lrc)),..Default::default()
    };
    h.frame(vec![]);
}

#[test]
fn switching_songs_clears_old_lyrics_immediately_even_while_a_request_is_running(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();lyrics_on(&mut h);
    let (release,id)=busy(&mut h);
    showing(&mut h,&song("QQMusic.exe","旧歌","歌手",200.0),"[00:01]旧歌词\n[00:05]第二句");
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("第二句"));
    let (late,rx)=std::sync::mpsc::channel();h.app.ly.rx=Some(rx);
    h.media=song("QQMusic.exe","新歌","歌手",180.0);h.frame(vec![]);
    assert!(h.app.ly.lyrics.is_none(),"换歌后不能继续显示上一首的歌词");
    assert!(h.app.island_lyric().is_none());
    assert!(h.app.ly.loading,"新歌应立即显示查找状态，不等旧请求结束");
    assert!(h.app.ly.key.starts_with("新歌|"));
    assert!(late.send(Ok((crate::lyrics::parse_lrc("[00:01]迟到的旧歌词"),false))).is_err(),"上一首迟到的结果必须丢弃");
    assert!(h.app.ly.pending.is_some() && h.app.ly.rx.is_none(),"在线查询排队，等旧请求结束");
    assert_eq!(h.app.ly_worker.as_ref().unwrap().thread().id(),id,"旧请求未结束时不得另开取词线程");
    finish(&mut h,release);
}

#[test]
fn filled_in_duration_keeps_synced_lyrics_without_a_new_lookup(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();lyrics_on(&mut h);
    let (release,_)=busy(&mut h);
    let mut media=song("QQMusic.exe","晴天","周杰伦",0.0);
    showing(&mut h,&media,"[00:01]故事的小黄花\n[00:05]从出生那年就飘着");
    media.duration=269.0;h.media=media.clone();h.frame(vec![]);
    assert_eq!(h.app.ly.key,"晴天|周杰伦|269");
    assert_eq!(h.app.ly.lyrics.as_ref().unwrap().lines[1].text,"从出生那年就飘着","补全时长不能清空歌词");
    assert!(!h.app.ly.loading && h.app.ly.pending.is_none(),"已有同步歌词时补全时长不再查询");
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("从出生那年就飘着"));
    // 歌手从空补全、写法变化同样视为同一首。
    let mut blank=song("QQMusic.exe","晴天","",269.0);showing(&mut h,&blank,"[00:05]从出生那年就飘着");
    blank.artist="周杰伦".into();h.media=blank;h.frame(vec![]);
    assert!(h.app.ly.lyrics.is_some() && h.app.ly.pending.is_none());
    finish(&mut h,release);
}

#[test]
fn completed_artist_credits_requery_but_keep_showing_current_lyrics(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();lyrics_on(&mut h);
    let (release,_)=busy(&mut h);
    let mut media=song("QQMusic.exe","有点甜","汪苏泷",230.0);
    showing(&mut h,&media,"[00:01]第一句\n[00:05]我的心");
    media.artist="汪苏泷、BY2".into();h.media=media;h.frame(vec![]);
    assert_eq!(h.app.ly.lyrics.as_ref().unwrap().lines[1].text,"我的心","查询期间继续显示已有歌词");
    assert!(!h.app.ly.loading,"已有歌词时不闪查找状态");
    assert!(h.app.ly.pending.is_some(),"署名补全后按完整署名重新查询");
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("我的心"));
    finish(&mut h,release);
}

#[test]
fn same_title_with_other_version_or_artist_is_a_different_song(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for next in [song("QQMusic.exe","Track","甲",300.0),song("QQMusic.exe","Track","丙",240.0),song("QQMusic.exe","Track (Live)","甲",240.0)] {
        let mut h=H::new();lyrics_on(&mut h);let (release,_)=busy(&mut h);
        showing(&mut h,&song("QQMusic.exe","Track","甲",240.0),"[00:01]原版歌词");
        h.media=next.clone();h.frame(vec![]);
        assert!(h.app.ly.lyrics.is_none(),"{} - {}（{} 秒）不能沿用原版歌词",next.title,next.artist,next.duration);
        finish(&mut h,release);
    }
}

#[test]
fn native_lyrics_polling_starts_on_switch_without_waiting_for_the_old_request(){
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    for source in ["汽水音乐","KuGou.exe"] {
        let mut h=H::new();lyrics_on(&mut h);let (release,_)=busy(&mut h);
        showing(&mut h,&song(source,"旧歌","歌手",200.0),"[00:01]旧歌词");
        let room=crate::resources::NATIVE_LYRICS.active()<2;
        h.media=song(source,"阶段二测试歌曲","不存在的歌手",180.0);h.frame(vec![]);
        if room {assert!(h.app.ly.native_rx.is_some(),"{source}：本机歌词轮询应立即开始");}
        assert!(h.app.ly.pending.is_some() && h.app.ly.rx.is_none(),"{source}：在线查询仍排队等待旧请求");
        // 本机歌词先到：排队的在线查询取消，不再联网。
        let (native,rx)=std::sync::mpsc::channel();h.app.ly.native_rx=Some(rx);
        native.send(Some(crate::lyrics::parse_lrc("[00:01]本机歌词"))).unwrap();h.frame(vec![]);
        assert_eq!(h.app.ly.lyrics.as_ref().unwrap().lines[0].text,"本机歌词");
        assert!(!h.app.ly.loading && h.app.ly.pending.is_none(),"{source}：本机歌词到达后取消排队的在线查询");
        finish(&mut h,release);
    }
}

#[test]
#[cfg(windows)]
fn player_version_without_progress_shows_the_title_instead_of_frozen_lyrics(){
    use crate::smtc::music_compat::{self as compat,Item,Support};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();lyrics_on(&mut h);
    let media=song("kugou","红玫瑰","陈奕迅",240.0);
    compat::set("KuGou.exe",Item::Progress,Support::Ok);
    showing(&mut h,&media,"[00:01]第一句\n[00:05]第二句");
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("第二句"));
    // 酷狗版本读不到进度：歌词无法同步，岛上改显示歌名，不显示停住的歌词。
    compat::set("KuGou.exe",Item::Progress,Support::Unavailable("版本不在适配表里".into()));
    h.frame(vec![]);
    assert!(h.app.music_lyrics_limited());
    assert_eq!(h.app.island_lyric(),None);
    // 拖动跳转不可用不影响歌词。
    compat::set("KuGou.exe",Item::Progress,Support::Learned);
    compat::set("KuGou.exe",Item::Seek,Support::Unavailable("接口不可用".into()));
    h.frame(vec![]);
    assert!(!h.app.music_lyrics_limited());
    assert_eq!(h.app.island_lyric().map(|(_,line)|line),Some("第二句"));
    compat::clear("KuGou.exe",Item::Progress);compat::clear("KuGou.exe",Item::Seek);
    // 其他播放器不受酷狗的结论影响。
    compat::set("KuGou.exe",Item::Progress,Support::Unavailable("x".into()));
    showing(&mut h,&song("QQMusic.exe","旧歌","歌手",200.0),"[00:01]旧歌词\n[00:05]第二句");
    assert!(!h.app.music_lyrics_limited());
    compat::clear("KuGou.exe",Item::Progress);
}

#[test]
#[cfg(windows)]
fn song_with_system_position_keeps_lyrics_after_player_fallback_fails(){
    use crate::smtc::music_compat::{self as compat,Item,Support};
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());
    let mut h=H::new();lyrics_on(&mut h);
    let (release,_)=busy(&mut h);
    compat::set("cloudmusic.exe",Item::Progress,Support::Unavailable("备用进度读取失败".into()));
    showing(&mut h,&song("cloudmusic.exe","缺少进度的歌","歌手",0.0),"[00:01]旧歌词");
    let without_position=h.app.island_lyric().is_none();
    let mut next=song("cloudmusic.exe","带系统进度的歌","歌手",220.0);
    next.sampled_at=Some(std::time::Instant::now());
    showing(&mut h,&next,"[00:01]新歌词");
    let with_position=h.app.island_lyric().map(|(_,line)|line.to_owned());
    compat::clear("cloudmusic.exe",Item::Progress);
    finish(&mut h,release);
    assert!(without_position,"读不到当前歌曲进度时应显示歌名");
    assert_eq!(with_position.as_deref(),Some("新歌词"));
}

#[test]
fn artist_filled_in_later_requeries_lyrics_with_the_full_credit(){
    // 波点：换歌后本机歌曲库约 30 秒才写入，先只有歌名，随后补上歌手和歌曲 ID。
    let _g=ENV_LOCK.lock().unwrap_or_else(|e|e.into_inner());let mut h=H::new();lyrics_on(&mut h);
    let (release,_)=busy(&mut h);
    let bare=song("bodian_pc.exe","微风细雨","",229.3);
    h.media=bare.clone();h.frame(vec![]);
    let first=h.app.ly.pending.is_some().then(||h.app.ly.song.as_ref().map(|song|(song.title.clone(),song.artist.clone()))).flatten();
    println!("只有歌名时排队的查询：{first:?}，key={}",h.app.ly.key);
    let mut full=bare.clone();full.artist="邓丽君".into();full.song_id=Some(27148558);
    h.media=full;h.frame(vec![]);
    let second=h.app.ly.pending.is_some().then(||h.app.ly.song.as_ref().map(|song|(song.title.clone(),song.artist.clone()))).flatten();
    println!("补上歌手后排队的查询：{second:?}，key={}",h.app.ly.key);
    assert_eq!(second,Some(("微风细雨".into(),"邓丽君".into())),"补上歌手后要按完整署名重新查询");
    finish(&mut h,release);
}
