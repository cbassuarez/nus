//! Rank actual settings rows. Every query token must match; short tokens
//! require exact words/prefixes, and typo correction never beats an exact label.
use super::*;
use crate::app::{Action,PaletteRow};

fn words(s:&str)->Vec<String> {
    s.to_lowercase().split(|c:char|!c.is_alphanumeric()).filter(|s|!s.is_empty()).map(str::to_owned).collect()
}

fn typo(a:&str,b:&str)->bool {
    let a:Vec<_>=a.chars().collect();let b:Vec<_>=b.chars().collect();
    if a.len()<4 || a.len().abs_diff(b.len())>1 {return false;}
    let mut d=vec![vec![0;b.len()+1];a.len()+1];
    for i in 0..=a.len() {d[i][0]=i;}
    for j in 0..=b.len() {d[0][j]=j;}
    for i in 1..=a.len() {for j in 1..=b.len() {
        d[i][j]=(d[i-1][j]+1).min(d[i][j-1]+1).min(d[i-1][j-1]+usize::from(a[i-1]!=b[j-1]));
        if i>1 && j>1 && a[i-1]==b[j-2] && a[i-2]==b[j-1] {d[i][j]=d[i][j].min(d[i-2][j-2]+1);}
    }}
    d[a.len()][b.len()]<=1
}

fn token_score(q:&str,w:&str)->i32 {
    if q==w {100} else if w.starts_with(q) {80} else if q.len()>=3 && w.contains(q) {55}
    else if typo(q,w) {45} else {0}
}

fn aliases(title:&str)->String {
    let title=title.to_lowercase();
    let mut out=String::new();
    for (terms,synonyms) in [
        ("recovery","rollback downgrade previous version restore"),
        ("support details","diagnostics version compatibility report"),
        ("location","place latitude longitude timezone privacy gps city"),
        ("font","typeface typography lettering family"),("weight","font bold regular medium thickness"),
        ("radius","rounded rounding corners carapace window"),("footer","theme switcher picker slots grid favorites favourites"),
        ("start page","new tab cmd t ctrl t launch default homepage"),("home address","homepage url website"),
        ("new window","cmd n ctrl n launch"),("reduce motion","animation accessibility movement"),
        ("replay","recording history timeline"),("phone","mobile remote access privacy"),
        ("copy selected","clipboard selection"),("clipboard","copy paste"),
        ("close","exit quit confirmation"),("sidebar","navigation side bar"),
        ("hatch","dropdown drop down terminal quake"),("on top","always above floating"),
        ("menu","menubar menu bar tray drawer signal desk modular desktop"),("signal","menubar menu bar tray dot badge count status"),
        ("quick actions","drawer desk shortcuts modules"),("finished items","drawer history completed recent"),
        ("idle","sleep suspend timeout"),("appearance","light dark paper ink"),
    ] {if title.contains(terms) {out.push_str(synonyms);out.push(' ');}}
    out
}

fn rank(query:&str,title:&str,body:&str,context:&str)->Option<i32> {
    let mut qs=words(query);
    if qs.len()>1 {qs.retain(|q|!matches!(q.as_str(),"the"|"my"|"your"|"a"|"an"|"do"|"how"|"i"|"to"|"for"|"set"|"change"|"where"|"can"|"is"));}
    if qs.is_empty() {return Some(0);}
    let label=words(title);let detail=words(body);let alias=words(&aliases(title));let context=words(context);
    let mut total=0;
    for q in &qs {
        let score=label.iter().map(|w|token_score(q,w)).max().unwrap_or(0)
            .max(alias.iter().map(|w|token_score(q,w)*8/10).max().unwrap_or(0))
            .max(detail.iter().map(|w|token_score(q,w)*6/10).max().unwrap_or(0))
            .max(context.iter().map(|w|token_score(q,w)*4/10).max().unwrap_or(0));
        if score==0 {return None;}total+=score;
    }
    if title.eq_ignore_ascii_case(query.trim()) {total+=250;}
    else if title.to_lowercase().contains(&query.trim().to_lowercase()) {total+=80;}
    Some(total)
}

fn contents(control:&Control)->String {
    match control {
        Control::Mercury => "Claim replay silver liquid metal app icon early edition".into(),
        Control::Info(s)|Control::Slider(_,_,s)=>s.clone(),
        Control::Choice(v)|Control::Strip(v)=>v.iter().map(|v|v.0.as_str()).collect::<Vec<_>>().join(" "),
        Control::Pics(v)=>v.iter().map(|v|format!("{} {}",v.0,v.1)).collect::<Vec<_>>().join(" "),
        Control::Actions(v)=>v.iter().map(|v|format!("{} {}",v.0,v.1)).collect::<Vec<_>>().join(" "),
        Control::Cards(v)=>v.iter().map(|v|v.0.as_str()).collect::<Vec<_>>().join(" "),
        Control::Buttons(v)=>v.iter().map(|v|v.0.as_str()).collect::<Vec<_>>().join(" "),
        Control::Art(v)=>v.iter().map(|v|format!("{} {}",v.1,v.2)).collect::<Vec<_>>().join(" "),
        Control::Keys(keys,note)=>format!("{} {}",keys.join(" "),note),
        _=>String::new(),
    }
}

impl App {
    pub(crate) fn search_settings(&self,query:&str)->Vec<PaletteRow> {
        let query:String=query.chars().take(160).collect();
        let query=query.as_str();
        let mut found=Vec::new();
        for (section,(name,_)) in SECTIONS.iter().enumerate() {
            for tab in 0..if section==SEC_LOOK {LOOK_TABS.len()} else {1} {
                let context=if section==SEC_LOOK {format!("{name} · {}",LOOK_TABS[tab])} else {name.to_string()};
                let rows=self.rows_for_at(section,tab);
                for (index,(title,control)) in rows.iter().enumerate() {
                    if matches!(control,Control::Studio|Control::Strip(_)|Control::Caption|Control::Section) {continue;}
                    let body=contents(control);
                    let title=if title.is_empty() {
                        if matches!(control,Control::Info(_)|Control::Proof(_)) {continue;}
                        body.split_whitespace().take(6).collect::<Vec<_>>().join(" ")
                    } else {title.clone()};
                    if title.is_empty() {continue;}
                    // Include adjacent explanatory copy with its actual control.
                    let note=rows.get(index+1).filter(|(s,_)|s.is_empty()).and_then(|(_,c)|if let Control::Info(s)=c {Some(s.as_str())}else{None}).unwrap_or("");
                    let body=format!("{body} {note}");
                    if let Some(mut score)=rank(query,&title,&body,&context) {
                        if matches!(control,Control::Info(_)|Control::Keys(..)) {score-=200;}
                        if title=="START PAGE" && matches!(query.trim().to_lowercase().as_str(),"new tab"|"newtab"|"default new tab") {score+=250;}
                        found.push((score,section,tab,index,format!("{title}  ·  {context}")));
                    }
                }
            }
        }
        found.sort_by(|a,b|b.0.cmp(&a.0).then_with(||a.1.cmp(&b.1)).then_with(||a.2.cmp(&b.2)).then_with(||a.3.cmp(&b.3)));
        if found.is_empty() {return vec![PaletteRow {num:"?".into(),text:"No matching settings. Try a feature, action, or shorter phrase.".into(),action:Action::Noop}];}
        let visible=((self.target.size.1 as f32*0.88-self.px(50.0))/self.px(34.0)).floor().clamp(1.0,10.0) as usize;
        found.into_iter().take(visible).map(|(_,sec,tab,row,text)|PaletteRow {num:"→".into(),text,action:Action::SettingsRow(sec,tab,row)}).collect()
    }
}

#[cfg(test)] mod tests {
    use super::*;
    #[test] fn matches_user_language_and_typing_errors() {
        for (query,title,context) in [("locatoin","Your location","Startup"),("new tab","Start page","Startup"),("rounded corners","Radius","Look"),("theme favorites","Footer theme slots","Look"),("bold interface","Interface weight","Look"),("font wieght","Terminal weight","Font") ,("side bar","Sidebar","Layout")] {
            assert!(rank(query,title,"",context).is_some(),"{query} → {title}");
        }
    }
    #[test] fn exact_labels_win_and_unrelated_words_do_not_leak() {
        assert!(rank("location","Your location","","Startup")>rank("location","Artwork","Set your location here","Startup"));
        assert!(rank("zzzz","Your location","Set latitude and longitude","Startup").is_none());
        assert!(rank("font banana","Font","Family and weight","Look").is_none());
        assert!(rank("on","Location","Latitude longitude","Startup").is_none());
    }
}
