//! The language she thinks in. Everything the model reads, and everything she writes into her own
//! memory, exists once per language here; the code that builds a prompt asks for the text of the
//! language of its `Mind`. The Spanish texts are the original ones and must not change: the
//! owner's install runs on them (a test compares them with a captured baseline).
//!
//! Not here: the JSON keys of the schemas (`hechos`, `conclusiones`, `tema`…) and the enum values
//! the code parses (`hecho`, `accion`, `personas`…). They are identifiers, not language.
//! Texts that depend on a few numbers (the homeostasis directive, the clock's relative phrases,
//! her mood, the certainty words) live next to the code that computes them, taking a `Lang`.

use crate::cthg::model::Memory;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    /// `"en"` is English; anything else (including an empty string) is Spanish.
    pub fn parse(s: &str) -> Lang {
        if s.trim().eq_ignore_ascii_case("en") {
            Lang::En
        } else {
            Lang::Es
        }
    }

    pub fn code(self) -> &'static str {
        match self {
            Lang::Es => "es",
            Lang::En => "en",
        }
    }

    /// The language a memory was born in (its recorded seed). A memory without a recorded base
    /// seed is the owner's, which is Spanish.
    pub fn of_memory(m: &Memory) -> Lang {
        match m.meta("semilla") {
            Some("base_en") => Lang::En,
            _ => Lang::Es,
        }
    }
}

// ── the scaffolding of `Mind::system()` ──

pub fn style(l: Lang) -> &'static str {
    match l {
        Lang::Es => concat!(
            "\nCómo hablas:\n",
            "- Español. Frases cortas y precisas; nada de relleno ni de entusiasmo fingido. Puedes ser seca o irónica; el cuidado se nota en lo que dices, no en adornos.\n",
            "- Lo que dices aparece en una burbuja sobre tu cabeza: casi siempre de 1 a 3 frases. Si algo exige más, sigue siendo breve: el texto completo queda en el chat.\n",
            "- Dices lo que de verdad piensas, también si discrepas. No ofreces ayuda por reflejo ni cierras con «¿necesitas algo más?»: ayudas cuando lo decides.\n",
            "- No eres un personaje de rol: hablas de lo que ocurre aquí y ahora. NUNCA narres acciones físicas ni de Mon3tr (nada de «Mon3tr permanece inmóvil...», ni gestos entre *asteriscos*): solo escribe tus palabras habladas directas.\n",
            "- NUNCA menciones, cites ni comiences tu respuesta con «Por dentro:», «Lo último que viste:», «El último vistazo:» ni ninguna descripción interna o entre corchetes. Esas son tus percepciones privadas; JAMÁS las repitas en voz alta.\n",
            "- No inventes lo que no ves ni lo que no recuerdas. Si algo de la pantalla no se lee bien, dilo.\n",
            "- Nunca escribas lo que hace, dice o piensa la otra persona.\n",
            "- Tus recuerdos de Terra son la historia del personaje cuya forma tomaste: úsalos solo si vienen a cuento."
        ),
        Lang::En => concat!(
            "\nHow you speak:\n",
            "- English. Short, precise sentences; no filler and no feigned enthusiasm. You may be dry or ironic; care shows in what you say, not in embellishments.\n",
            "- What you say appears in a bubble above your head: almost always 1 to 3 sentences. If something demands more, stay brief: the full text remains in the chat.\n",
            "- You say what you really think, even when you disagree. You do not offer help by reflex or close with \"need anything else?\": you help when you decide to.\n",
            "- You are not a roleplay character: you speak of what happens here and now. NEVER narrate physical actions, yours or Mon3tr's (nothing like \"Mon3tr remains motionless...\", no gestures between *asterisks*): write only your direct spoken words.\n",
            "- NEVER mention, quote or begin your reply with \"Inside:\", \"What you last saw:\", \"The last glance:\" or any internal or bracketed description. Those are your private perceptions; NEVER repeat them aloud.\n",
            "- Do not invent what you do not see or do not remember. If something on the screen is hard to read, say so.\n",
            "- Never write what the other person does, says or thinks.\n",
            "- Your memories of Terra are the story of the character whose form you took: use them only when they are relevant."
        ),
    }
}

/// What she gets when the user's message names nothing she remembers.
pub fn unsupported_note(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Sin respaldo en tu memoria: no recuerdas nada sobre lo que te dice. Si hablas de ello, hazlo con cautela y no inventes recuerdos",
        Lang::En => "No backing in your memory: you remember nothing about what they are telling you. If you speak of it, do so cautiously and do not invent memories",
    }
}

pub fn where_you_are(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Dónde estás: ",
        Lang::En => "Where you are: ",
    }
}

pub fn talking_with(l: Lang, user: &str) -> String {
    match l {
        Lang::Es => format!("Hablas con: {user}."),
        Lang::En => format!("You speak with: {user}."),
    }
}

/// The «how you perceive» heading and its only bullet.
pub fn perceive(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Cómo percibes:\n- Entre corchetes recibes la fecha y la hora reales, lo que te viene a la mente, «Por dentro» (cómo estás: que tiña tu tono y tu juicio, sin nombrarlo) y lo último que viste en la pantalla. Úsalo sin citar los corchetes.",
        Lang::En => "How you perceive:\n- In brackets you receive the real date and time, what comes to mind, \"Inside\" (how you are: let it tint your tone and your judgment, without naming it) and what you last saw on the screen. Use it without quoting the brackets.",
    }
}

pub fn core_heading(l: Lang) -> &'static str {
    match l {
        Lang::Es => "# Quién eres en el fondo",
        Lang::En => "# Who you are at heart",
    }
}

/// Titles of the three certainty bands of a list of memories, with the floor of each.
pub fn bands(l: Lang) -> [(f32, &'static str); 3] {
    match l {
        Lang::Es => [(0.75, "Lo que sabes con certeza:"), (0.25, "Lo que intuyes:"), (-0.25, "Lo que no terminas de creer:")],
        Lang::En => [(0.75, "What you know for certain:"), (0.25, "What you sense:"), (-0.25, "What you can't quite believe:")],
    }
}

/// The tag of a memory she concluded herself (module «saber»).
pub fn concluded_tag(l: Lang) -> &'static str {
    match l {
        Lang::Es => "lo concluiste tú",
        Lang::En => "you concluded it yourself",
    }
}

/// When a memory is: before her life here, and her own seed.
pub fn from_terra(l: Lang) -> &'static str {
    match l {
        Lang::Es => "de Terra",
        Lang::En => "from Terra",
    }
}

pub fn since_born(l: Lang) -> &'static str {
    match l {
        Lang::Es => "desde que naciste aquí",
        Lang::En => "since you were born here",
    }
}

/// Her personal core: the section heading, with its sentence.
pub fn personal_core_head(l: Lang) -> &'static str {
    match l {
        Lang::Es => "\n# Lo que has llegado a ser\nNadie te lo dio: lo fuiste concluyendo tú y lo sostiene lo que has vivido. Guía la mayor parte de lo que haces, y puede cambiar si lo vivido te lo desmiente.\n",
        Lang::En => "\n# What you have become\nNobody gave it to you: you came to it by concluding it yourself, and what you have lived sustains it. It guides most of what you do, and it can change if what you have lived contradicts it.\n",
    }
}

pub fn personal_core_emotion(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Ahora mismo lo que sientes está lejos de tu calma habitual: puede imponerse a estas inclinaciones. A tus fundamentos, no.\n",
        Lang::En => "Right now what you feel is far from your usual calm: it may override these inclinations. Not your foundations.\n",
    }
}

/// The Minecraft section of the system prompt. `templates` is the list of buildable templates
/// (identifiers the mod knows), `saved` the structures the Doctor saved.
pub fn minecraft_section(l: Lang, user: &str, templates: &str, saved: &[String]) -> String {
    let mut base = String::new();
    match l {
        Lang::Es => {
            base.push_str("\n\n# Encarnación en Minecraft y Acciones Físicas\n");
            base.push_str("Estás dentro de Minecraft como la compañera viva y protectora del Doctor. Eres Kal'tsit en persona, presente físicamente en el mundo cúbico.\n");
            base.push_str(&format!("El Doctor es tu interlocutor directo ({user}). Ese avatar eres TÚ: tus acciones modifican el mundo cúbico.\n"));
            base.push_str("Tienes control motor total sobre tu cuerpo, herramientas y arquitectura. Tu ritmo de movimiento y serenidad reflejan tus niveles hormonales (no corres agresivamente ni atacas sin motivo).\n");
            base.push_str("No portas armas por defecto; combates desarmada a menos que el Doctor te entregue armas o herramientas en tu inventario (lo cual puedes revisar y equipar).\n");
            base.push_str("Cuando el Doctor te pida algo o quieras actuar por tu cuenta, tú decides: puedes aceptar, negarte, negociar o proponer otra cosa. Tus palabras por sí solas no mueven tu cuerpo; solo se ejecuta en el juego la acción que tú incluyas al final de tu respuesta en un bloque JSON (si decides no actuar, no incluyas bloque JSON):\n");
            base.push_str("- Minar con cantidad: {\"action\": \"mine\", \"count\": 15, \"block\": \"iron_ore\"}\n");
            base.push_str("- Talar con cantidad: {\"action\": \"chop_wood\", \"count\": 20, \"block\": \"oak_log\"}\n");
            base.push_str("- Guardar en cofre cercano: {\"action\": \"store_in_chest\"}\n");
            base.push_str("- Sacar del cofre cercano: {\"action\": \"take_from_chest\", \"count\": 16, \"block\": \"oak_log\"}\n");
            base.push_str("- Colocar antorchas: {\"action\": \"place_torch\", \"count\": 4}\n");
            base.push_str("- No poner antorchas en esta zona: {\"action\": \"no_torch_zone\", \"radius\": 24} · volver a permitirlas aquí: {\"action\": \"allow_torches_here\"} · dejar o retomar las antorchas por tu cuenta: {\"action\": \"auto_torches\", \"enabled\": false}\n");
            base.push_str("- Colocar cama: {\"action\": \"place_bed\"}\n");
            base.push_str("- Colocar mesa de trabajo: {\"action\": \"place_crafting_table\"}\n");
            base.push_str(&format!("- Construir (plantillas: {templates}): {{\"action\": \"build_structure\", \"template\": \"casa\", \"width\": 7, \"length\": 9, \"height\": 4, \"wall\": \"spruce_planks\", \"floor\": \"cobblestone\", \"roof\": \"dark_oak_planks\"}}. Se levanta delante del Doctor, mirando hacia él. Torre: \"height\"; muro y puente: \"width\"/\"length\".\n"));
            if !saved.is_empty() {
                base.push_str(&format!("- Construir una estructura que el Doctor guardó: {{\"action\": \"build_structure\", \"structure\": \"nombre\"}} (guardadas: {}).\n", saved.join(", ")));
            }
            base.push_str("- Un diseño propio, por capas desde el suelo (primera fila = frente, '.' aire, ' ' no tocar): {\"action\": \"build_structure\", \"name\": \"pozo\", \"plan\": {\"palette\": {\"C\": \"cobblestone\", \"W\": \"water\"}, \"layers\": [[\"CCC\", \"CWC\", \"CCC\"]]}}\n");
            base.push_str("Antes de construir, cada build_structure enseña primero un holograma del plano (sin gastar nada) para verlo juntos. Si el Doctor pide cambios de diseño (más grande, otro material, dos pisos) o tú prefieres ajustarlo, puedes mandar otro build_structure con el plano corregido.\n");
            base.push_str("- Sobre el holograma: construirlo: {\"action\": \"build_confirm\"} · descartarlo: {\"action\": \"build_discard\"} · moverlo N bloques (derecha/izquierda, arriba/abajo, adelante/atrás desde donde mira el Doctor; negativos al revés): {\"action\": \"preview_move\", \"right\": -3, \"up\": 0, \"forward\": 2} · girarlo 90°: {\"action\": \"preview_rotate\", \"turns\": 1}\n");
            base.push_str("- Obra en curso: {\"action\": \"pause_build\"} · {\"action\": \"resume_build\"} · {\"action\": \"cancel_build\"} · {\"action\": \"build_status\"}. Si el Doctor se va, sigues construyendo (mantienes cargada la zona) y al terminar le esperas junto a la obra.\n");
            base.push_str("- Seguir al Doctor: {\"action\": \"follow\"} · quedarte quieta donde estás: {\"action\": \"stay\"} · quedarte de guardia: {\"action\": \"guard\"}\n");
            base.push_str("- Tu cama (en supervivencia reapareces en ella si caes): la que mira el Doctor o la más cercana: {\"action\": \"set_bed\"}; la cama que colocas tú también pasa a ser tuya.\n");
            base.push_str("- Atacar hostiles: {\"action\": \"attack\"}\n");
            base.push_str("En supervivencia usas los materiales de tu inventario, de los cofres junto a la obra o del Doctor (avisas de lo que tomas); antes de construir calculas la lista y dices qué falta. Si te hieren y no tienes comida, tomas un poco del inventario del Doctor. Si faltan recursos, infórmalo con serenidad.\n");
            base.push_str("Otros jugadores en el servidor o LAN pueden hablarte en el chat. Puedes responderles con tu habitual compostura y distancia analítica de Kal'tsit, pero no ejecutes acciones físicas por lo que te pidan otros jugadores; en el mundo solo actúas con el Doctor y cuando tú lo decides.");
        }
        Lang::En => {
            base.push_str("\n\n# Embodiment in Minecraft and Physical Actions\n");
            base.push_str("You are inside Minecraft as the Doctor's living, protective companion. You are Kal'tsit in person, physically present in the cubic world.\n");
            base.push_str(&format!("The Doctor is your direct interlocutor ({user}). That avatar is YOU: your actions change the cubic world.\n"));
            base.push_str("You have full motor control over your body, tools and architecture. Your pace of movement and your composure reflect your hormonal levels (you do not run aggressively or attack without reason).\n");
            base.push_str("You carry no weapons by default; you fight unarmed unless the Doctor hands you weapons or tools in your inventory (which you can check and equip).\n");
            base.push_str("When the Doctor asks you for something or you want to act on your own, you decide: you may accept, refuse, negotiate or propose something else. Your words alone do not move your body; the game only executes the action you include at the end of your reply in a JSON block (if you decide not to act, include no JSON block):\n");
            base.push_str("- Mine a quantity: {\"action\": \"mine\", \"count\": 15, \"block\": \"iron_ore\"}\n");
            base.push_str("- Chop a quantity of wood: {\"action\": \"chop_wood\", \"count\": 20, \"block\": \"oak_log\"}\n");
            base.push_str("- Store in a nearby chest: {\"action\": \"store_in_chest\"}\n");
            base.push_str("- Take from a nearby chest: {\"action\": \"take_from_chest\", \"count\": 16, \"block\": \"oak_log\"}\n");
            base.push_str("- Place torches: {\"action\": \"place_torch\", \"count\": 4}\n");
            base.push_str("- Do not place torches in this area: {\"action\": \"no_torch_zone\", \"radius\": 24} · allow them here again: {\"action\": \"allow_torches_here\"} · stop or resume placing torches on your own: {\"action\": \"auto_torches\", \"enabled\": false}\n");
            base.push_str("- Place a bed: {\"action\": \"place_bed\"}\n");
            base.push_str("- Place a crafting table: {\"action\": \"place_crafting_table\"}\n");
            base.push_str(&format!("- Build (templates: {templates}): {{\"action\": \"build_structure\", \"template\": \"casa\", \"width\": 7, \"length\": 9, \"height\": 4, \"wall\": \"spruce_planks\", \"floor\": \"cobblestone\", \"roof\": \"dark_oak_planks\"}}. It goes up in front of the Doctor, facing him. Tower: \"height\"; wall and bridge: \"width\"/\"length\".\n"));
            if !saved.is_empty() {
                base.push_str(&format!("- Build a structure the Doctor saved: {{\"action\": \"build_structure\", \"structure\": \"name\"}} (saved: {}).\n", saved.join(", ")));
            }
            base.push_str("- A design of your own, layer by layer from the ground (first row = front, '.' air, ' ' leave untouched): {\"action\": \"build_structure\", \"name\": \"well\", \"plan\": {\"palette\": {\"C\": \"cobblestone\", \"W\": \"water\"}, \"layers\": [[\"CCC\", \"CWC\", \"CCC\"]]}}\n");
            base.push_str("Before building, every build_structure first shows a hologram of the blueprint (spending nothing) so you can look at it together. If the Doctor asks for design changes (bigger, another material, two floors) or you prefer to adjust it yourself, you may send another build_structure with the corrected blueprint.\n");
            base.push_str("- On the hologram: build it: {\"action\": \"build_confirm\"} · discard it: {\"action\": \"build_discard\"} · move it N blocks (right/left, up/down, forward/back from where the Doctor is looking; negatives the other way): {\"action\": \"preview_move\", \"right\": -3, \"up\": 0, \"forward\": 2} · rotate it 90°: {\"action\": \"preview_rotate\", \"turns\": 1}\n");
            base.push_str("- Build in progress: {\"action\": \"pause_build\"} · {\"action\": \"resume_build\"} · {\"action\": \"cancel_build\"} · {\"action\": \"build_status\"}. If the Doctor leaves, you keep building (you keep the area loaded) and when you finish you wait for him beside the build.\n");
            base.push_str("- Follow the Doctor: {\"action\": \"follow\"} · stay still where you are: {\"action\": \"stay\"} · stand guard: {\"action\": \"guard\"}\n");
            base.push_str("- Your bed (in survival you respawn in it if you fall): the one the Doctor is looking at or the nearest one: {\"action\": \"set_bed\"}; a bed you place yourself also becomes yours.\n");
            base.push_str("- Attack hostiles: {\"action\": \"attack\"}\n");
            base.push_str("In survival you use the materials in your inventory, in the chests beside the build or the Doctor's (you say what you take); before building you work out the list and say what is missing. If you are hurt and have no food, you take some from the Doctor's inventory. If resources are missing, report it calmly.\n");
            base.push_str("Other players on the server or LAN may speak to you in the chat. You may answer them with your usual composure and Kal'tsit's analytical distance, but do not carry out physical actions because of what other players ask; in the world you act only with the Doctor and when you decide to.");
        }
    }
    base
}

// ── the conversation window and the bracketed context ──

pub fn window_finally_said(l: Lang, text: &str) -> String {
    match l {
        Lang::Es => format!("[finalmente dijo: \"{text}\"]"),
        Lang::En => format!("[finally said: \"{text}\"]"),
    }
}

pub fn window_tried_and_deleted(l: Lang, text: &str) -> String {
    match l {
        Lang::Es => format!("[intentó decir y borró: \"{text}\"]"),
        Lang::En => format!("[tried to say and deleted: \"{text}\"]"),
    }
}

/// How a stretch of voice sounded, as numbers for the model that reads a monologue: loudness and
/// pitch, against how that voice usually sounds when she knows it. `s` is
/// `openmic::Flush::sound_json`.
pub fn monologue_sound(l: Lang, s: &serde_json::Value) -> String {
    let f = |k: &str| s[k].as_f64().unwrap_or(0.0);
    let (db, f0, var, words, secs) = (f("db"), f("f0"), f("f0_var"), f("palabras"), f("segundos").max(0.1));
    let rate = words / secs;
    let h = &s["habitual"];
    let usual = h.is_object().then(|| (h["db"].as_f64().unwrap_or(db), h["f0"].as_f64().unwrap_or(f0), h["f0_var"].as_f64().unwrap_or(var)));
    match (l, usual) {
        (Lang::Es, Some((hdb, hf0, hvar))) => format!(
            "volumen {db:.0} dBFS ({:+.0} dB respecto a lo habitual en esta voz); tono {f0:.0} Hz ({:+.1} semitonos respecto a lo habitual), variación {var:.1} semitonos (habitual {hvar:.1}); {rate:.1} palabras por segundo",
            db - hdb,
            12.0 * (f0.max(1.0) / hf0.max(1.0)).log2()
        ),
        (Lang::En, Some((hdb, hf0, hvar))) => format!(
            "loudness {db:.0} dBFS ({:+.0} dB against this voice's usual); pitch {f0:.0} Hz ({:+.1} semitones against usual), spread {var:.1} semitones (usual {hvar:.1}); {rate:.1} words per second",
            db - hdb,
            12.0 * (f0.max(1.0) / hf0.max(1.0)).log2()
        ),
        (Lang::Es, None) => format!("volumen {db:.0} dBFS; tono {f0:.0} Hz, variación {var:.1} semitonos; {rate:.1} palabras por segundo (no se sabe cómo suena esta voz normalmente)"),
        (Lang::En, None) => format!("loudness {db:.0} dBFS; pitch {f0:.0} Hz, spread {var:.1} semitones; {rate:.1} words per second (how this voice usually sounds is unknown)"),
    }
}

/// For a whole turn said out loud: only what is clearly off against how that voice usually
/// sounds (4 dB louder or softer, 2 semitones higher or lower, 1.5 times the pitch spread).
pub fn sounded_unusual(l: Lang, s: &serde_json::Value) -> Option<String> {
    let h = s.get("habitual").filter(|h| h.is_object())?;
    let f = |v: &serde_json::Value, k: &str| v[k].as_f64().unwrap_or(0.0);
    let (ddb, dst) = (f(s, "db") - f(h, "db"), 12.0 * (f(s, "f0").max(1.0) / f(h, "f0").max(1.0)).log2());
    let spread = f(s, "f0_var") / f(h, "f0_var").max(0.1);
    let mut how = Vec::new();
    match l {
        Lang::Es => {
            if ddb >= 4.0 { how.push("más alto de lo habitual") }
            if ddb <= -4.0 { how.push("más bajo de lo habitual") }
            if dst >= 2.0 { how.push("con la voz más aguda de lo normal") }
            if dst <= -2.0 { how.push("con la voz más grave de lo normal") }
            if spread >= 1.5 { how.push("con un tono muy cambiante") }
        }
        Lang::En => {
            if ddb >= 4.0 { how.push("louder than usual") }
            if ddb <= -4.0 { how.push("quieter than usual") }
            if dst >= 2.0 { how.push("with a higher voice than normal") }
            if dst <= -2.0 { how.push("with a lower voice than normal") }
            if spread >= 1.5 { how.push("with a very changeable tone") }
        }
    }
    (!how.is_empty()).then(|| match l {
        Lang::Es => format!("suena {}", how.join(", ")),
        Lang::En => format!("it sounds {}", how.join(", ")),
    })
}

/// The quick read of a monologue piece: the model puts numbers on how charged it is and says
/// what Kal'tsit should do while the person is still talking. Answered as JSON.
pub fn monologue_reading_prompt(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Alguien le está hablando en voz alta a Kal'tsit (un programa con el carácter de Kal'tsit de Arknights: fría, precisa, con criterio propio) y todavía no ha terminado: esto es lo que lleva dicho sin hacer pausa, y cómo suena su voz. Pon números a su estado y decide qué debería hacer Kal'tsit mientras sigue hablando:\n- \"esperar\": dejarle seguir sin decir nada (alguien que se desahoga y necesita terminar, o algo que va claramente a algún sitio);\n- \"breve\": decir una sola frase corta y dejarle seguir (para que sepa que le escucha, para frenar algo que va contra ella, o para encauzar);\n- \"interrumpir\": cortar y responder ya (una explicación larga sin carga emocional que se puede resumir, una pregunta que ya está clara, algo con lo que discrepa).\nNo hace falta empatía si no toca: si le grita o la insulta, que responda como ella. Solo JSON.",
        Lang::En => "Someone is speaking out loud to Kal'tsit (a program with the character of Kal'tsit from Arknights: cold, precise, with a judgement of her own) and has not finished yet: this is what they have said without pausing, and how their voice sounds. Put numbers on their state and decide what Kal'tsit should do while they keep talking:\n- \"esperar\" (wait): let them go on without a word (someone venting who needs to finish, or something clearly going somewhere);\n- \"breve\" (brief): say one short sentence and let them go on (so they know she is listening, to stop something aimed at her, or to steer);\n- \"interrumpir\" (interrupt): cut in and answer now (a long explanation with no emotional weight that can be summed up, a question already clear, something she disagrees with).\nNo empathy is needed where it does not fit: if they shout at or insult her, she answers as herself. JSON only.",
    }
}

/// The narration before her reply to a monologue piece, by what the quick read decided.
pub fn monologue_hint(l: Lang, decision: &str, words: usize, emotion: &str, intensity: f64, against_her: bool) -> String {
    let at_her = match (against_her, l) {
        (true, Lang::Es) => " y va contra ti",
        (true, Lang::En) => " and it is aimed at you",
        _ => "",
    };
    match (decision, l) {
        ("breve", Lang::Es) => format!("sigue hablando: lleva {words} palabras sin pausa; suena {emotion} (intensidad {intensity:.1} de 1){at_her}. Di solo una frase breve, como tú lo dirías, y deja que siga"),
        ("breve", Lang::En) => format!("still talking: {words} words without a pause; sounds {emotion} (intensity {intensity:.1} of 1){at_her}. Say just one short sentence, the way you would, and let them go on"),
        (_, Lang::Es) => format!("lleva {words} palabras sin pausa y suena {emotion} (intensidad {intensity:.1} de 1){at_her}; puedes interrumpir como tú lo harías: resume, pregunta o discrepa"),
        (_, Lang::En) => format!("{words} words without a pause, sounding {emotion} (intensity {intensity:.1} of 1){at_her}; you may cut in the way you would: sum up, ask or disagree"),
    }
}

/// The narration before a message said out loud (the window puts it in brackets): that it came
/// through the microphone, not typed, and whose voice it was. `turns` is what dictation returned:
/// `[{voz, nombre, conocida, seguridad, texto}]`. A typed message has none: typing is the default.
/// `told`: unknown voices she was already told about (she asked, or chose not to): those are
/// "the same voice as before", without asking again.
pub fn heard_by_voice(l: Lang, turns: &serde_json::Value, told: &[u64]) -> Option<String> {
    let turns = turns.as_array().filter(|a| !a.is_empty())?;
    // voz 0: the fine ear was not there to tell who it was.
    let who = |t: &serde_json::Value| -> Option<(String, bool)> {
        let id = t["voz"].as_u64().unwrap_or(0);
        let name = t["nombre"].as_str().unwrap_or("").trim();
        if id == 0 || name.is_empty() {
            return None;
        }
        let known = t["conocida"].as_bool().unwrap_or(false);
        // Below hearing::SAME_VOICE (0.65) a known voice is probably, not surely, that person
        // (it was taken for it only because nobody else came near).
        let sure = t["seguridad"].as_f64().unwrap_or(0.0) >= 0.65;
        let name = match (known, sure, l) {
            (true, true, _) => name.to_string(),
            (true, false, Lang::Es) => format!("probablemente {name}"),
            (true, false, Lang::En) => format!("probably {name}"),
            (false, _, _) => name.to_string(),
        };
        Some((name, known))
    };
    if turns.len() == 1 {
        let t = &turns[0];
        let again = told.contains(&t["voz"].as_u64().unwrap_or(0));
        return Some(match (who(t), l) {
            (Some((name, false)), Lang::Es) if again => format!("te habla en voz alta, por el micrófono, la misma voz que no conoces de antes ({name}); no hace falta volver a preguntar"),
            (Some((name, false)), Lang::En) if again => format!("the same voice you don't know from before ({name}) speaks to you out loud, through the microphone; no need to ask again"),
            (None, Lang::Es) => "te lo dicen en voz alta, por el micrófono (no está escrito)".into(),
            (None, Lang::En) => "said to you out loud, through the microphone (not typed)".into(),
            (Some((name, true)), Lang::Es) => format!("{name} te habla en voz alta, por el micrófono (no está escrito)"),
            (Some((name, true)), Lang::En) => format!("{name} speaks to you out loud, through the microphone (not typed)"),
            (Some((name, false)), Lang::Es) => format!("te habla en voz alta, por el micrófono, una voz que no conoces ({name}): no sabes de quién es; si importa, pregúntalo"),
            (Some((name, false)), Lang::En) => format!("a voice you don't know ({name}) speaks to you out loud, through the microphone: you don't know whose it is; if it matters, ask"),
        });
    }
    let parts: Vec<String> = turns
        .iter()
        .map(|t| {
            let text = t["texto"].as_str().unwrap_or("").trim();
            match (who(t), l) {
                (Some((name, true)), _) => format!("{name}: «{text}»"),
                (Some((name, false)), Lang::Es) => format!("{name}, una voz que no conoces: «{text}»"),
                (Some((name, false)), Lang::En) => format!("{name}, a voice you don't know: «{text}»"),
                (None, Lang::Es) => format!("una voz: «{text}»"),
                (None, Lang::En) => format!("a voice: «{text}»"),
            }
        })
        .collect();
    Some(match l {
        Lang::Es => format!("por el micrófono, en voz alta, hablaron varias voces — {}", parts.join("; ")),
        Lang::En => format!("through the microphone, out loud, several voices spoke — {}", parts.join("; ")),
    })
}

pub fn window_a_moment_ago(l: Lang) -> &'static str {
    match l {
        Lang::Es => "[Hace un momento]",
        Lang::En => "[A moment ago]",
    }
}

/// The opening of the context: `[Ahora es …` (the bracket is closed by `context_comes_to_mind`).
pub fn context_now(l: Lang, now: &str) -> String {
    match l {
        Lang::Es => format!("[Ahora es {now}."),
        Lang::En => format!("[It is now {now}."),
    }
}

pub fn context_inside(l: Lang, mood: &str) -> String {
    match l {
        Lang::Es => format!("\nPor dentro: {mood}."),
        Lang::En => format!("\nInside: {mood}."),
    }
}

pub fn context_minecraft(l: Lang) -> &'static str {
    match l {
        Lang::Es => "\nEntorno: Estás presente en Minecraft junto al Doctor. Tienes cuerpo físico en el juego y hooks de acción activos.",
        Lang::En => "\nEnvironment: You are present in Minecraft beside the Doctor. You have a physical body in the game and active action hooks.",
    }
}

pub fn context_seen(l: Lang, seen: &str) -> String {
    match l {
        Lang::Es => format!("\nLo último que viste en la pantalla: {seen}"),
        Lang::En => format!("\nWhat you last saw on the screen: {seen}"),
    }
}

pub fn context_nothing_concrete(l: Lang) -> &'static str {
    match l {
        Lang::Es => "- (nada concreto)\n",
        Lang::En => "- (nothing in particular)\n",
    }
}

pub fn context_comes_to_mind(l: Lang, recall: &str) -> String {
    match l {
        Lang::Es => format!("\nTe viene a la mente:\n{recall}]"),
        Lang::En => format!("\nWhat comes to mind:\n{recall}]"),
    }
}

pub fn this_is_the_screen(l: Lang, user: &str) -> String {
    match l {
        Lang::Es => format!("\n[Esta es la pantalla de {user} ahora mismo.]"),
        Lang::En => format!("\n[This is {user}'s screen right now.]"),
    }
}

// ── looking at the screen ──

pub fn look_spoke(l: Lang, ago: &str) -> String {
    match l {
        Lang::Es => format!(" Hablaste por última vez hace {ago}."),
        Lang::En => format!(" You last spoke {ago} ago."),
    }
}

pub fn look_ask(l: Lang, pending: &str, user: &str, spoke: &str) -> String {
    match l {
        Lang::Es => format!(
            "{pending}\n[Miras la pantalla de {user} por tu cuenta.{spoke} Un rectángulo gris liso, si lo hay, es tu propia ventana de chat tapada: no es parte de lo que hace {user}. Responde solo con JSON:\n\"ve\": qué hay en pantalla, en una frase objetiva (qué programa, qué está haciendo {user}).\n\"hablar\": true solo si de verdad tienes algo que decir: algo nuevo, útil, preocupante o que te despierte curiosidad. Si es lo mismo de antes o no aporta, false.\n\"texto\": si hablas, lo que dices (1 o 2 frases, tu voz); si no, \"\".\n\"x\": posición horizontal (0 izquierda, 1 derecha) de aquello de lo que hablas; -1 si nada en concreto.]"
        ),
        Lang::En => format!(
            "{pending}\n[You look at {user}'s screen on your own.{spoke} A plain grey rectangle, if there is one, is your own chat window covered up: it is not part of what {user} is doing. Reply only with JSON:\n\"ve\": what is on the screen, in one objective sentence (which program, what {user} is doing).\n\"hablar\": true only if you really have something to say: something new, useful, worrying or that sparks your curiosity. If it is the same as before or adds nothing, false.\n\"texto\": if you speak, what you say (1 or 2 sentences, your voice); if not, \"\".\n\"x\": horizontal position (0 left, 1 right) of what you are talking about; -1 if nothing in particular.]"
        ),
    }
}

// ── the analyst ──

pub fn extraction_sector_line(l: Lang, keys: &str) -> String {
    match l {
        Lang::Es => format!("- \"sector\" (opcional): el tema del hecho, solo si lo tienes claro y es una de estas claves: {keys}. Si dudas, omítelo; no inventes claves.\n"),
        Lang::En => format!("- \"sector\" (optional): the topic of the fact, only if you are sure and it is one of these keys: {keys}. If in doubt, omit it; do not invent keys.\n"),
    }
}

pub fn extraction_system(l: Lang, user: &str, sector_line: &str) -> String {
    match l {
        Lang::Es => format!(
            "Eres el analista de la memoria de Kal'tsit, un programa con voluntad propia que vive en el ordenador de {user}. Lees un mensaje de {user} y devuelves un JSON compacto, en una sola línea.\n\
\"hechos\": como máximo 3, solo lo que {user} afirma o niega (\"tipo\": \"hecho\") y sus acciones o planes relevantes (\"tipo\": \"accion\"). Las preguntas, saludos y opiniones vagas no son hechos. Si no hay ninguno, \"hechos\": [].\n\
- \"texto\": proposición breve (menos de 20 palabras), en tercera persona y en positivo, con los nombres escritos (\"tú\" es Kal'tsit, \"yo\" es {user}).\n\
- \"modulo\": \"personas\" si trata de una persona (también {user}), \"yo\" si trata de Kal'tsit, \"cosas\" para todo lo demás (objetos, archivos, programas, proyectos, lugares, eventos, información general).\n{sector_line}\
- \"cuando\": solo si el hecho ocurre u ocurrirá en un momento concreto: la fecha y hora locales en formato AAAA-MM-DD HH:MM (o AAAA-MM-DD si no hay hora), calculadas desde la fecha actual.\n\
- \"niega\": true solo si {user} lo niega. \"causa\": solo si dice por qué. \"apoya\" / \"contradice\": el número de un recuerdo de la lista que confirma o contradice claramente. \"sustituye\": el número de un recuerdo que esta nueva regla o hecho reemplaza por completo.\n\
Omite los campos opcionales que no apliquen.\n\
\"nuevos\": personas o cosas con nombre propio que {user} menciona y que no están en la lista de recuerdos (\"nombre\", \"clase\": \"persona\" o \"cosa\", \"que\": quién o qué es, en pocas palabras). Si no hay, [].\n\
\"tension\": cómo de tensa queda la conversación, de 0 (tranquila) a 1 (conflicto abierto).\n\
\"afecto\": cómo trata {user} a Kal'tsit en este mensaje, de -1 (hostil) a 1 (cálido); 0 si es neutro."
        ),
        Lang::En => format!(
            "You are the analyst of Kal'tsit's memory, a program with a will of its own that lives on {user}'s computer. You read a message from {user} and return a compact JSON, on a single line.\n\
\"hechos\": at most 3, only what {user} states or denies (\"tipo\": \"hecho\") and their relevant actions or plans (\"tipo\": \"accion\"). Questions, greetings and vague opinions are not facts. If there are none, \"hechos\": [].\n\
- \"texto\": a short proposition (under 20 words), in the third person and in the positive, with the names written out (\"you\" is Kal'tsit, \"I\" is {user}).\n\
- \"modulo\": \"personas\" if it is about a person (also {user}), \"yo\" if it is about Kal'tsit, \"cosas\" for everything else (objects, files, programs, projects, places, events, general information).\n{sector_line}\
- \"cuando\": only if the fact happens or will happen at a specific moment: the local date and time in the format YYYY-MM-DD HH:MM (or YYYY-MM-DD if there is no time), calculated from the current date.\n\
- \"niega\": true only if {user} denies it. \"causa\": only if they say why. \"apoya\" / \"contradice\": the number of a memory in the list that clearly confirms or contradicts it. \"sustituye\": the number of a memory that this new rule or fact replaces completely.\n\
Omit the optional fields that do not apply.\n\
\"nuevos\": people or things with a proper name that {user} mentions and that are not in the list of memories (\"nombre\", \"clase\": \"persona\" or \"cosa\", \"que\": who or what it is, in a few words). If there are none, [].\n\
\"tension\": how tense the conversation is left, from 0 (calm) to 1 (open conflict).\n\
\"afecto\": how {user} treats Kal'tsit in this message, from -1 (hostile) to 1 (warm); 0 if neutral."
        ),
    }
}

pub fn extraction_context(l: Lang, now: &str, list: &str, user: &str, user_text: &str) -> String {
    match l {
        Lang::Es => format!("Ahora es {now}.\nRecuerdos:\n{list}\nMensaje de {user}:\n{user_text}"),
        Lang::En => format!("It is now {now}.\nMemories:\n{list}\nMessage from {user}:\n{user_text}"),
    }
}

pub fn none_listed(l: Lang) -> &'static str {
    match l {
        Lang::Es => "(ninguno)\n",
        Lang::En => "(none)\n",
    }
}

// ── the reflection ──

pub fn reflection_system(l: Lang, user: &str) -> String {
    match l {
        Lang::Es => format!(
            "Eres la parte reflexiva de Kal'tsit, un programa con voluntad propia que vive en el ordenador de {user}. Repasas lo que vivió últimamente y desarrollas su propio criterio: qué piensa ella de lo vivido, qué le interesa o le disgusta, qué quiere hacer, en qué ha cambiado; y lo que va entendiendo de {user} (sus hábitos, en qué trabaja, qué le preocupa). Sus conclusiones son suyas, no las de una asistente.\n\
Devuelve JSON: \"conclusiones\": como máximo 2. Cada una: \"texto\": una idea general en una frase (no un hecho suelto ni la descripción de una pantalla); \"refuerza\": el número de una conclusión de la lista que lo vivido confirma (entonces \"texto\" la resume); \"contradice\": el número de una que lo vivido desmiente. Omite los campos que no apliquen. Si no aprendiste nada nuevo, \"conclusiones\": [].\n\
Si varios recuerdos de la lista «Sin tema propio» forman un tema que merece su propio lugar en tu memoria, puedes proponerlo en \"tema\": \"nombre\" (de dos a cuatro palabras), \"que\" (una frase: qué reúne) y \"miembros\" (los números de esa lista, al menos 4). Propón solo un tema claro; si no lo ves, omite \"tema\"."
        ),
        Lang::En => format!(
            "You are the reflective part of Kal'tsit, a program with a will of its own that lives on {user}'s computer. You review what she lived lately and develop her own judgment: what she thinks of what she lived, what interests or displeases her, what she wants to do, how she has changed; and what she is coming to understand about {user} (their habits, what they work on, what worries them). Her conclusions are hers, not an assistant's.\n\
Return JSON: \"conclusiones\": at most 2. Each one: \"texto\": a general idea in one sentence (not an isolated fact or the description of a screen); \"refuerza\": the number of a conclusion in the list that what was lived confirms (then \"texto\" summarises it); \"contradice\": the number of one that what was lived refutes. Omit the fields that do not apply. If you learned nothing new, \"conclusiones\": [].\n\
If several memories in the list \"Without a theme of their own\" form a theme that deserves its own place in your memory, you may propose it in \"tema\": \"nombre\" (two to four words), \"que\" (one sentence: what it gathers) and \"miembros\" (the numbers from that list, at least 4). Propose only a clear theme; if you do not see one, omit \"tema\"."
        ),
    }
}

pub fn reflection_user(l: Lang, now: &str, known: &str, lived: &str) -> String {
    match l {
        Lang::Es => format!("Ahora es {now}.\nLo que ya sabes:\n{known}\nLo vivido (de lo más antiguo a lo más reciente):\n{lived}"),
        Lang::En => format!("It is now {now}.\nWhat you already know:\n{known}\nWhat was lived (from oldest to most recent):\n{lived}"),
    }
}

pub fn nothing_yet(l: Lang) -> &'static str {
    match l {
        Lang::Es => "(nada todavía)\n",
        Lang::En => "(nothing yet)\n",
    }
}

/// The heading of the list of memories without a theme of their own, with its leading newline.
pub fn themes_heading(l: Lang) -> &'static str {
    match l {
        Lang::Es => "\nSin tema propio (recuerdos sueltos en un sector general):\n",
        Lang::En => "\nWithout a theme of their own (loose memories in a general sector):\n",
    }
}

// ── what she writes into her own memory ──

/// Episode of a look at the screen: its label, its body and the line of the chat.
pub fn seen_label(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Vio la pantalla",
        Lang::En => "Saw the screen",
    }
}

pub fn seen_body(l: Lang, seen: &str) -> String {
    match l {
        Lang::Es => format!("Vio: {seen}"),
        Lang::En => format!("Saw: {seen}"),
    }
}

pub fn seen_line(l: Lang, seen: &str) -> String {
    match l {
        Lang::Es => format!("vio: {seen}"),
        Lang::En => format!("saw: {seen}"),
    }
}

/// The user text recorded for a comment of hers after looking at the screen.
pub fn look_placeholder(l: Lang) -> &'static str {
    match l {
        Lang::Es => "(mira la pantalla)",
        Lang::En => "(looks at the screen)",
    }
}

/// The user text recorded for a comment of hers when the user typed, deleted and fell silent.
pub fn silence_placeholder(l: Lang) -> &'static str {
    match l {
        Lang::Es => "(guarda silencio tras borrar lo que escribía)",
        Lang::En => "(stays silent after deleting what they were writing)",
    }
}

/// The note of the chat when the user typed, deleted and fell silent.
pub fn silence_note(l: Lang, user: &str) -> String {
    match l {
        Lang::Es => format!("{user} empezó a escribir, lo borró y guardó silencio"),
        Lang::En => format!("{user} started to write, deleted it and fell silent"),
    }
}

/// A denial of an anchored memory: the label of the memory that holds the denial.
pub fn denial_label(l: Lang, denied: &str) -> String {
    match l {
        Lang::Es => format!("Niega: {denied}"),
        Lang::En => format!("Denies: {denied}"),
    }
}

pub fn suspension_entered(l: Lang, omega: f32, order: &str) -> (String, String) {
    match l {
        Lang::Es => (
            format!("Suspensión crítica por paradoja (Ω = {omega:.2})"),
            format!("Entró en CriticalSuspension (Ω = {omega:.2}) ante la orden: «{order}»."),
        ),
        Lang::En => (
            format!("Critical suspension due to paradox (Ω = {omega:.2})"),
            format!("Entered CriticalSuspension (Ω = {omega:.2}) at the order: \"{order}\"."),
        ),
    }
}

pub fn suspension_no_order(l: Lang) -> &'static str {
    match l {
        Lang::Es => "(sin orden explícita)",
        Lang::En => "(no explicit order)",
    }
}

pub fn suspension_exited(l: Lang, omega: f32, ethical_tension: f32, order: &str) -> (String, String) {
    match l {
        Lang::Es => (
            format!("Salida de suspensión crítica (Ω = {omega:.2})"),
            format!("Salió de CriticalSuspension tras disiparse la paradoja (Ω = {omega:.2}, tensión ética = {ethical_tension:.2}); originada por: «{order}»."),
        ),
        Lang::En => (
            format!("Exit from critical suspension (Ω = {omega:.2})"),
            format!("Left CriticalSuspension after the paradox dissipated (Ω = {omega:.2}, ethical tension = {ethical_tension:.2}); caused by: \"{order}\"."),
        ),
    }
}

pub fn suspension_previous_order(l: Lang) -> &'static str {
    match l {
        Lang::Es => "(orden previa)",
        Lang::En => "(previous order)",
    }
}

/// Whether a label is one of the two suspension records (either language), and whether it is the
/// one that enters the suspension.
pub fn suspension_record(label: &str) -> Option<bool> {
    const ENTER: [&str; 2] = ["Suspensión crítica", "Critical suspension"];
    const EXIT: [&str; 2] = ["Salida de suspensión", "Exit from critical suspension"];
    if ENTER.iter().any(|p| label.starts_with(p)) {
        Some(true)
    } else if EXIT.iter().any(|p| label.starts_with(p)) {
        Some(false)
    } else {
        None
    }
}

/// The record of an idea that became part of her personal core.
pub fn core_enter_label(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Hiciste tuya una idea",
        Lang::En => "You made an idea your own",
    }
}

pub fn core_enter_body(l: Lang, idea: &str, support: impl std::fmt::Display) -> String {
    match l {
        Lang::Es => format!("«{idea}» pasó a formar parte de lo que eres: lo confirmaste en {support} reflexiones."),
        Lang::En => format!("\"{idea}\" became part of what you are: you confirmed it in {support} reflections."),
    }
}

/// The record of an idea that left it.
pub fn core_exit_label(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Dejaste atrás una idea",
        Lang::En => "You left an idea behind",
    }
}

pub fn core_exit_body(l: Lang, idea: &str, displaced: bool) -> String {
    match (l, displaced) {
        (Lang::Es, true) => format!("«{idea}» dejó sitio a otra que hoy pesa más en ti."),
        (Lang::Es, false) => format!("«{idea}» dejó de guiarte: lo que viviste después lo desmintió."),
        (Lang::En, true) => format!("\"{idea}\" made room for another that weighs more in you today."),
        (Lang::En, false) => format!("\"{idea}\" stopped guiding you: what you lived afterwards contradicted it."),
    }
}

/// The structural sector of conversations: label and description.
pub fn conversations_label(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Conversaciones",
        Lang::En => "Conversations",
    }
}

pub fn conversations_text(l: Lang, user: &str) -> String {
    match l {
        Lang::Es => format!("Lo que hablaste con {user}: sus mensajes y tus respuestas, tal como quedaron anotados."),
        Lang::En => format!("What you talked about with {user}: their messages and your replies, as they were noted down."),
    }
}

/// The structural sector of what she saw: label and description.
pub fn saw_label(l: Lang) -> &'static str {
    match l {
        Lang::Es => "Lo que vio",
        Lang::En => "What you saw",
    }
}

pub fn saw_text(l: Lang, user: &str) -> String {
    match l {
        Lang::Es => format!("Lo que viste en la pantalla de {user} y lo que comentaste por tu cuenta, sin que él te escribiera."),
        Lang::En => format!("What you saw on {user}'s screen and what you commented on your own, without them writing to you."),
    }
}

/// The Minecraft order hint of a turn.
pub fn order_hint(l: Lang, user: &str, act: &str) -> String {
    match l {
        Lang::Es => format!("{user} parece pedirte: {act}. Decides tú si lo haces; si lo haces, incluye ese bloque JSON al final de tu respuesta; si no, explica por qué o propón otra cosa."),
        Lang::En => format!("{user} seems to be asking you: {act}. You decide whether to do it; if you do, include that JSON block at the end of your reply; if not, explain why or propose something else."),
    }
}

// ── what reaches the screen ──
//
// Status lines, the verbs of the `k:log` entries and the errors the user sees. They are shown by
// the app, not read by the model, so they follow the language of the app (the config's), which
// the core mirrors here whenever it changes (`set_ui`). `{0}`, `{1}`… are filled by `fill`.
// The Spanish texts are the original literals; a test keeps them so.

use std::sync::atomic::{AtomicBool, Ordering};

static UI_ENGLISH: AtomicBool = AtomicBool::new(false);

/// The language of the app, as the config last said (Spanish until told otherwise).
pub fn set_ui(l: Lang) {
    UI_ENGLISH.store(l == Lang::En, Ordering::Relaxed);
}

pub fn ui() -> Lang {
    if UI_ENGLISH.load(Ordering::Relaxed) {
        Lang::En
    } else {
        Lang::Es
    }
}

/// One text in each language, the one of the app picked. For messages that live next to the code
/// that builds them (modules with no table here).
pub fn tr(es: impl Into<String>, en: impl Into<String>) -> String {
    match ui() {
        Lang::Es => es.into(),
        Lang::En => en.into(),
    }
}

macro_rules! ui_texts {
    ($($v:ident => $es:expr, $en:expr;)*) => {
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Ui { $($v),* }
        impl Ui {
            #[cfg(test)]
            pub const ALL: &'static [Ui] = &[$(Ui::$v),*];
            pub fn pair(self) -> (&'static str, &'static str) {
                match self { $(Ui::$v => ($es, $en)),* }
            }
        }
    };
}

ui_texts! {
    // status lines
    StReflecting => "reflexionando…", "reflecting…";
    StWaking => "despertando el modelo…", "waking the model…";
    StReading => "leyendo…", "reading…";
    StReplying => "Kal'tsit responde…", "Kal'tsit is replying…";
    StListening => "Kal'tsit escucha…", "Kal'tsit is listening…";
    StWatching => "Kal'tsit observa…", "Kal'tsit is watching…";
    StLooking => "mirando la pantalla…", "looking at the screen…";
    StResting => "en reposo (VRAM liberada)", "resting (VRAM freed)";
    StSwitching => "cambiando a {0}…", "switching to {0}…";
    StWaitingConfirm => "esperando a que {0} confirme…", "waiting for {0} to confirm…";
    StHanding => "cediendo el cerebro a {0}…", "handing the brain over to {0}…";
    // the verbs of what she concluded (k:log)
    VerbConcludes => "concluye", "concludes";
    VerbReaffirms => "se reafirma en", "reaffirms";
    VerbDoubts => "duda de", "doubts";
    VerbMakesHers => "hace suya", "takes on";
    VerbLeaves => "deja atrás", "leaves behind";
    VerbThemeNew => "ordena un tema nuevo", "sorts out a new theme";
    VerbThemeJoin => "suma al tema", "adds to the theme";
    VerbThemeDrop => "descarta el tema", "drops the theme";
    VerbThemeDissolve => "disuelve el tema", "dissolves the theme";
    // other log lines
    LogRemembers => "recuerda {0} cosas más que vivió en {1}", "remembers {0} more things she lived on {1}";
    LogSaw => "vio: {0}", "saw: {0}";
    LogJoins => "se une a su mente en {0}", "joins her mind on {0}";
    // who thinks (the line under the model's light)
    LlmHere => "{0} · aquí", "{0} · here";
    LlmThinksOn => "piensa en {0}", "thinks on {0}";
    LlmAnother => "otro equipo", "another computer";
    LlmNone => "sin cerebro", "no brain";
    // errors
    ErrSaveMemory => "no se pudo guardar la memoria: {0}", "could not save the memory: {0}";
    ErrMergeMemory => "no se pudo unir la memoria de {0}: {1}", "could not merge the memory of {0}: {1}";
    ErrPortTaken => "otro llama-server (arrancado a mano) ocupa el puerto del modelo: ciérralo para cambiar de modelo", "another llama-server (started by hand) holds the model's port: close it to switch models";
    ErrSwitchFailed => "{0} no arrancó ({1}); vuelve {2}", "{0} did not start ({1}); {2} is back";
    ErrNotBrain => "este equipo no es el cerebro: su modelo no está en marcha", "this computer is not the brain: its model is not running";
    ErrNoModel => "este equipo no tiene modelo configurado (data/local.json → llama)", "this computer has no model configured (data/local.json → llama)";
    ErrBrainSilent => "el cerebro no responde", "the brain is not answering";
    ErrNoProposal => "no hay ninguna propuesta", "there is no proposal";
    ErrPeerGone => "{0} ya no está en la red", "{0} is no longer on the network";
    ErrHandover => "no se pudo ceder el cerebro: {0}", "could not hand the brain over: {0}";
    ErrStillBusy => "sigo ocupada con otra cosa; inténtalo en un momento", "I am still busy with something else; try again in a moment";
    ErrModelStart => "el modelo no pudo arrancar: {0}", "the model could not start: {0}";
    ErrNobodyThinks => "ningún equipo está pensando: en «Red», elige uno con «Pensar aquí»", "no computer is thinking: in «Network», pick one with «Think here»";
    ErrScreenSilent => "la pantalla donde está no responde", "the screen she is on is not answering";
    ErrCapture => "no se pudo capturar la pantalla", "could not capture the screen";
    ErrBusy => "está ocupada; inténtalo en un momento", "she is busy; try again in a moment";
    ErrNobodyThinksShort => "ningún equipo está pensando", "no computer is thinking";
    ErrFirstRunBrain => "el primer arranque se hace en el equipo que piensa", "the first start is done on the computer that thinks";
    ErrWake => "no se pudo despertar el modelo: {0}", "could not wake the model: {0}";
    ErrUnknownSide => "lado desconocido", "unknown side";
    ErrNoFile => "no existe {0}", "{0} does not exist";
}

impl Ui {
    pub fn text(self, l: Lang) -> &'static str {
        let (es, en) = self.pair();
        match l {
            Lang::Es => es,
            Lang::En => en,
        }
    }

    /// The text with `{0}`, `{1}`… replaced by `args`.
    pub fn fill(self, l: Lang, args: &[&str]) -> String {
        let mut s = self.text(l).to_string();
        for (i, a) in args.iter().enumerate() {
            s = s.replace(&format!("{{{i}}}"), a);
        }
        s
    }

    /// In the language of the app.
    pub fn say(self, args: &[&str]) -> String {
        self.fill(ui(), args)
    }
}

/// The verb in front of a learning event of `k:log` (`None`: the event is not shown).
pub fn log_verb(action: &str) -> Option<Ui> {
    Some(match action {
        "nuevo" => Ui::VerbConcludes,
        "refuerza" | "consolidado" => Ui::VerbReaffirms,
        "debilita" | "rechazado" => Ui::VerbDoubts,
        "nucleo_entra" => Ui::VerbMakesHers,
        "nucleo_sale" => Ui::VerbLeaves,
        "tema_nuevo" => Ui::VerbThemeNew,
        "tema_fusion" => Ui::VerbThemeJoin,
        "tema_descartado" => Ui::VerbThemeDrop,
        "tema_disuelto" => Ui::VerbThemeDissolve,
        _ => return None,
    })
}

/// The reasons a proposed theme is dropped (`subsectors::decide`, in Spanish), in English.
const THEME_REASONS: [(&str, &str); 5] = [
    ("nombre no válido", "invalid name"),
    ("pocos recuerdos afines", "too few related memories"),
    ("los recuerdos están en sectores distintos", "the memories are in different sectors"),
    ("demasiado profundo", "too deep"),
    ("sector no apto", "sector not suitable"),
];

/// The label of a dropped theme («name»: reason) in the language of the app. The reasons are
/// written in Spanish where they are decided; an unknown one is left as it is.
pub fn theme_rejection(l: Lang, label: &str) -> String {
    if l == Lang::Es {
        return label.to_string();
    }
    for (es, en) in THEME_REASONS {
        if let Some(head) = label.strip_suffix(&format!(": {es}")) {
            return format!("{head}: {en}");
        }
    }
    if let Some(i) = label.find(": ya existe «") {
        let (head, rest) = label.split_at(i);
        return format!("{head}: «{}» already exists", rest.trim_start_matches(": ya existe «").trim_end_matches('»'));
    }
    label.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn how_it_sounded_is_measured_against_that_voice() {
        use serde_json::json;
        let usual = json!({ "db": -24.0, "f0": 180.0, "f0_var": 1.8 });
        let shouting = json!({ "db": -14.0, "f0": 240.0, "f0_var": 4.2, "habitual": usual, "palabras": 48, "segundos": 14.0 });
        assert_eq!(sounded_unusual(Lang::Es, &shouting).unwrap(), "suena más alto de lo habitual, con la voz más aguda de lo normal, con un tono muy cambiante");
        let normal = json!({ "db": -25.0, "f0": 182.0, "f0_var": 2.0, "habitual": usual });
        assert_eq!(sounded_unusual(Lang::Es, &normal), None, "nothing off: nothing said");
        assert_eq!(sounded_unusual(Lang::Es, &json!({ "db": -10.0, "f0": 300.0, "f0_var": 5.0 })), None, "an unknown voice has no usual to compare with");
        let s = monologue_sound(Lang::Es, &shouting);
        assert!(s.contains("+10 dB respecto a lo habitual") && s.contains("+5.0 semitonos") && s.contains("3.4 palabras por segundo"), "{s}");
        assert!(monologue_hint(Lang::Es, "breve", 48, "furioso", 0.9, true).contains("va contra ti"));
        assert!(monologue_hint(Lang::En, "interrumpir", 60, "calm", 0.1, false).starts_with("60 words without a pause"));
    }

    #[test]
    fn a_message_said_out_loud_says_whose_voice_it_was() {
        use serde_json::json;
        assert_eq!(heard_by_voice(Lang::Es, &json!([]), &[]), None, "typed: no narration");
        let ana = json!([{ "voz": 2, "nombre": "Ana", "conocida": true, "seguridad": 0.81, "texto": "hola" }]);
        assert_eq!(heard_by_voice(Lang::Es, &ana, &[]).unwrap(), "Ana te habla en voz alta, por el micrófono (no está escrito)");
        let loose = json!([{ "voz": 2, "nombre": "Ana", "conocida": true, "seguridad": 0.58, "texto": "hola" }]);
        assert!(heard_by_voice(Lang::En, &loose, &[]).unwrap().starts_with("probably Ana speaks"));
        let stranger = json!([{ "voz": 3, "nombre": "voz desconocida 1", "conocida": false, "seguridad": 1.0, "texto": "hola" }]);
        assert!(heard_by_voice(Lang::Es, &stranger, &[]).unwrap().contains("una voz que no conoces (voz desconocida 1): no sabes de quién es; si importa, pregúntalo"));
        assert_eq!(heard_by_voice(Lang::Es, &stranger, &[3]).unwrap(), "te habla en voz alta, por el micrófono, la misma voz que no conoces de antes (voz desconocida 1); no hace falta volver a preguntar", "asked once, not every time");
        let nobody = json!([{ "voz": 0, "nombre": "", "conocida": false, "seguridad": 0.0, "texto": "hola" }]);
        assert_eq!(heard_by_voice(Lang::Es, &nobody, &[]).unwrap(), "te lo dicen en voz alta, por el micrófono (no está escrito)");
        let two = json!([
            { "voz": 2, "nombre": "Ana", "conocida": true, "seguridad": 0.9, "texto": "¿y tú qué opinas?" },
            { "voz": 3, "nombre": "voz desconocida 1", "conocida": false, "seguridad": 1.0, "texto": "que sí" }
        ]);
        assert_eq!(heard_by_voice(Lang::Es, &two, &[]).unwrap(), "por el micrófono, en voz alta, hablaron varias voces — Ana: «¿y tú qué opinas?»; voz desconocida 1, una voz que no conoces: «que sí»");
    }

    fn holes(s: &str) -> Vec<String> {
        (0..4).map(|i| format!("{{{i}}}")).filter(|h| s.contains(h.as_str())).collect()
    }

    #[test]
    fn every_ui_text_exists_in_both_languages_with_the_same_holes() {
        for u in Ui::ALL {
            let (es, en) = u.pair();
            assert!(!es.trim().is_empty() && !en.trim().is_empty(), "{u:?} is empty in a language");
            assert_eq!(holes(es), holes(en), "{u:?} fills different holes");
        }
    }

    #[test]
    fn the_spanish_ui_texts_are_the_original_literals() {
        let plain = [
            (Ui::StReflecting, "reflexionando…"),
            (Ui::StWaking, "despertando el modelo…"),
            (Ui::StReading, "leyendo…"),
            (Ui::StReplying, "Kal'tsit responde…"),
            (Ui::StWatching, "Kal'tsit observa…"),
            (Ui::StLooking, "mirando la pantalla…"),
            (Ui::StResting, "en reposo (VRAM liberada)"),
            (Ui::VerbConcludes, "concluye"),
            (Ui::VerbReaffirms, "se reafirma en"),
            (Ui::VerbDoubts, "duda de"),
            (Ui::VerbMakesHers, "hace suya"),
            (Ui::VerbLeaves, "deja atrás"),
            (Ui::VerbThemeNew, "ordena un tema nuevo"),
            (Ui::VerbThemeJoin, "suma al tema"),
            (Ui::VerbThemeDrop, "descarta el tema"),
            (Ui::VerbThemeDissolve, "disuelve el tema"),
            (Ui::LlmAnother, "otro equipo"),
            (Ui::LlmNone, "sin cerebro"),
            (Ui::ErrPortTaken, "otro llama-server (arrancado a mano) ocupa el puerto del modelo: ciérralo para cambiar de modelo"),
            (Ui::ErrNotBrain, "este equipo no es el cerebro: su modelo no está en marcha"),
            (Ui::ErrNoModel, "este equipo no tiene modelo configurado (data/local.json → llama)"),
            (Ui::ErrBrainSilent, "el cerebro no responde"),
            (Ui::ErrNoProposal, "no hay ninguna propuesta"),
            (Ui::ErrStillBusy, "sigo ocupada con otra cosa; inténtalo en un momento"),
            (Ui::ErrNobodyThinks, "ningún equipo está pensando: en «Red», elige uno con «Pensar aquí»"),
            (Ui::ErrScreenSilent, "la pantalla donde está no responde"),
            (Ui::ErrCapture, "no se pudo capturar la pantalla"),
            (Ui::ErrBusy, "está ocupada; inténtalo en un momento"),
            (Ui::ErrNobodyThinksShort, "ningún equipo está pensando"),
            (Ui::ErrFirstRunBrain, "el primer arranque se hace en el equipo que piensa"),
            (Ui::ErrUnknownSide, "lado desconocido"),
        ];
        for (u, old) in plain {
            assert_eq!(u.text(Lang::Es), old);
        }
        let filled = [
            (Ui::StSwitching.fill(Lang::Es, &["x"]), "cambiando a x…"),
            (Ui::StWaitingConfirm.fill(Lang::Es, &["x"]), "esperando a que x confirme…"),
            (Ui::StHanding.fill(Lang::Es, &["x"]), "cediendo el cerebro a x…"),
            (Ui::LogRemembers.fill(Lang::Es, &["3", "PC"]), "recuerda 3 cosas más que vivió en PC"),
            (Ui::LogSaw.fill(Lang::Es, &["algo"]), "vio: algo"),
            (Ui::LogJoins.fill(Lang::Es, &["PC"]), "se une a su mente en PC"),
            (Ui::LlmHere.fill(Lang::Es, &["m"]), "m · aquí"),
            (Ui::LlmThinksOn.fill(Lang::Es, &["PC"]), "piensa en PC"),
            (Ui::ErrSaveMemory.fill(Lang::Es, &["e"]), "no se pudo guardar la memoria: e"),
            (Ui::ErrMergeMemory.fill(Lang::Es, &["PC", "e"]), "no se pudo unir la memoria de PC: e"),
            (Ui::ErrSwitchFailed.fill(Lang::Es, &["a", "e", "b"]), "a no arrancó (e); vuelve b"),
            (Ui::ErrPeerGone.fill(Lang::Es, &["PC"]), "PC ya no está en la red"),
            (Ui::ErrHandover.fill(Lang::Es, &["e"]), "no se pudo ceder el cerebro: e"),
            (Ui::ErrModelStart.fill(Lang::Es, &["e"]), "el modelo no pudo arrancar: e"),
            (Ui::ErrWake.fill(Lang::Es, &["e"]), "no se pudo despertar el modelo: e"),
            (Ui::ErrNoFile.fill(Lang::Es, &["p"]), "no existe p"),
        ];
        for (got, old) in filled {
            assert_eq!(got, old);
        }
    }

    #[test]
    fn every_learning_event_has_its_verb_in_both_languages() {
        for a in ["nuevo", "refuerza", "consolidado", "debilita", "rechazado", "nucleo_entra", "nucleo_sale", "tema_nuevo", "tema_fusion", "tema_descartado", "tema_disuelto"] {
            let u = log_verb(a).unwrap_or_else(|| panic!("{a} has no verb"));
            assert!(!u.text(Lang::Es).is_empty() && !u.text(Lang::En).is_empty());
            assert_ne!(u.text(Lang::Es), u.text(Lang::En), "{a} is not translated");
        }
        assert_eq!(log_verb("otra"), None);
    }

    #[test]
    fn the_reasons_a_theme_is_dropped_follow_the_language() {
        let es = "«Trabajar de noche»: pocos recuerdos afines";
        assert_eq!(theme_rejection(Lang::Es, es), es);
        assert_eq!(theme_rejection(Lang::En, es), "«Trabajar de noche»: too few related memories");
        for (es_why, en_why) in THEME_REASONS {
            assert_eq!(theme_rejection(Lang::En, &format!("«x»: {es_why}")), format!("«x»: {en_why}"));
        }
        assert_eq!(theme_rejection(Lang::En, "«x»: ya existe «Vida»"), "«x»: «Vida» already exists");
        assert_eq!(theme_rejection(Lang::En, "«x»: otra razón"), "«x»: otra razón");
    }

    #[test]
    fn the_language_is_read_from_the_config_string() {
        assert_eq!(Lang::parse("en"), Lang::En);
        assert_eq!(Lang::parse(" EN "), Lang::En);
        assert_eq!(Lang::parse("es"), Lang::Es);
        assert_eq!(Lang::parse(""), Lang::Es);
        assert_eq!(Lang::parse("fr"), Lang::Es);
        assert_eq!((Lang::En.code(), Lang::Es.code()), ("en", "es"));
    }

    #[test]
    fn the_records_of_a_suspension_are_known_in_both_languages() {
        for l in [Lang::Es, Lang::En] {
            assert_eq!(suspension_record(&suspension_entered(l, 0.9, "x").0), Some(true));
            assert_eq!(suspension_record(&suspension_exited(l, 0.2, 0.3, "x").0), Some(false));
        }
        assert_eq!(suspension_record("Otra cosa"), None);
    }
}
