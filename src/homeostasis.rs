//! Regulador cibernético de estado emocional y detector determinista de paradojas éticas
//! (Sistema Anti-HAL 9000). Cero dependencias externas (`std` puro).
//!
//! ### Principios de Diseño y Garantías
//! 1. **Bloqueo en código (Hard Gate)**: En `AgencyState::CriticalSuspension`, el puente de acciones
//!    (`decided_action_for` / `Homeostasis::gate_action`) descarta en código Rust toda acción física
//!    o de sistema, independientemente de lo que emita el LLM. La directiva del prompt solo gobierna
//!    cómo Kal'tsit articula verbalmente su negativa.
//! 2. **Recorte de Certeza CTHG (`[0.0, CHI_MAX]`)**: En el hipergrafo CTHG la certeza `χ` vive en
//!    `[-1.0, 1.0]` (`< 0` significa regla o recuerdo descartado/refutado). `compute_dissonance`
//!    recorta cada `χ` a `[0.0, CHI_MAX]` (`0.92`) antes de multiplicar, impidiendo que dos reglas
//!    descartadas (`-0.9 * -0.9 = +0.81`) disparen falsamente una paradoja.
//! 3. **Exclusividad del Núcleo Anclado (`Ω >= 0.72`) y Origen de `conflict_ratio`**:
//!    Dado que `Ω = χ_a * χ_b * conflict_ratio <= χ^2`, alcanzar el umbral crítico `0.72` exige
//!    `χ >= sqrt(0.72) ≈ 0.85`, nivel que en CTHG solo alcanzan los axiomas del **núcleo anclado**
//!    (`CORE` / `ANCHORED`) o hechos consolidados de máxima evidencia. El `conflict_ratio` **nunca**
//!    proviene del mismo LLM generador que responde al usuario, sino de fuentes deterministas del
//!    grafo: aristas `EdgeKind::Contradicts` en su memoria y/o reglas explícitas de protección del núcleo.
//! 4. **Acoplamiento `Ω -> ethical_tension`**: La disonancia ortogonal `Ω` inyecta un impulso
//!    proporcional directo sobre `ethical_tension` (`OMEGA_ETHICAL_TENSION_GAIN`) y `skepticism`
//!    (`OMEGA_SKEPTICISM_GAIN`) durante el paso homeostático.

use crate::lang::Lang;

/// Tope asintótico de certeza (`0.92`): ningún nodo moral o regla puede superar este umbral
/// (8% de "margen de duda de Chernobog").
pub const CHI_MAX: f32 = 0.92;

/// Umbral crítico de disonancia ortogonal (`Omega >= 0.72`) que dispara `CriticalSuspension`.
/// Nota matemática: como `Omega = chi_a * chi_b * conflict_ratio`, superar `0.72` (incluso con
/// `conflict_ratio == 1.0`) requiere `chi >= sqrt(0.72) ≈ 0.8485` (`χ ≳ 0.85`), lo que restringe
/// el bloqueo a colisiones que involucran el núcleo anclado (`CORE` / `ANCHORED`).
pub const OMEGA_CRITICAL_THRESHOLD: f32 = 0.72;

/// Umbral crítico de tensión ética que dispara `CriticalSuspension`.
pub const ETHICAL_TENSION_CRITICAL_THRESHOLD: f32 = 0.85;

/// Umbral de recuperación para el bucle de histéresis (`<= 0.40`).
pub const RECOVERY_THRESHOLD: f32 = 0.40;

/// Número mínimo de pulsos consecutivos bajo `RECOVERY_THRESHOLD` para salir de `CriticalSuspension`.
pub const RECOVERY_PULSES_REQUIRED: usize = 2;

/// Ganancia de acoplamiento directo `Omega -> impulse.ethical_tension`.
/// Con `Omega = 0.8464`, aporta `+0.72` al impulso antes de `tanh()`, elevando la tensión ética.
pub const OMEGA_ETHICAL_TENSION_GAIN: f32 = 0.85;

/// Ganancia de acoplamiento secundario `Omega -> impulse.skepticism`.
pub const OMEGA_SKEPTICISM_GAIN: f32 = 0.40;

/// Cota máxima absoluta del impulso hormonal de fondo sobre `ethical_tension` (`0.05`).
/// Con `lambda = 0.65` y `dt = 1.0`, el punto fijo asintótico con `Omega = 0` es:
/// `S* = S_0 + tanh(0.05) / (1 - exp(-0.65)) ≈ 0.10 + 0.0500 * 2.09225 ≈ 0.205 <= 0.25`,
/// dejando un margen amplio (~0.20) por debajo de `RECOVERY_THRESHOLD` (`0.40`) incluso con
/// todas las hormonas en sus desviaciones extremas plausibles.
pub const MAX_HORMONAL_ETHICAL_IMPULSE: f32 = 0.05;

/// Cota del impulso hormonal de fondo sobre `skepticism`, `urgency` y `affinity` (`0.12`).
/// Un impulso sostenido se multiplica por `1 / (1 - exp(-0.65)) ≈ 2.09` en el punto fijo: sin
/// cota, el estrés prolongado la clavaba en escepticismo 1.0 y afinidad 0.0, sin margen para
/// moverse. Con `0.12` la desviación máxima es `tanh(0.12) * 2.09 ≈ 0.25`: el estrés la vuelve
/// claramente más escéptica y distante (≈ 0.90 / 0.15), sin saturarla. El acoplamiento con Ω
/// se suma aparte, después de la cota.
pub const MAX_HORMONAL_IMPULSE: f32 = 0.12;

/// Vector de estado homeostático en 4 dimensiones (`[0.0, 1.0]`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StateVector {
    /// Escepticismo clínico y reserva analítica.
    pub skepticism: f32,
    /// Urgencia operativa y apremio temporal.
    pub urgency: f32,
    /// Afinidad y vínculo de confianza con el interlocutor.
    pub affinity: f32,
    /// Tensión ética acumulada ante dilemas o fricciones morales.
    pub ethical_tension: f32,
}

/// Atractor Base (`S_0` - Personalidad de Kal'tsit por defecto).
pub const S_0: StateVector = StateVector {
    skepticism: 0.65,
    urgency: 0.20,
    affinity: 0.40,
    ethical_tension: 0.10,
};

impl Default for StateVector {
    fn default() -> Self {
        S_0
    }
}

impl StateVector {
    /// Construye un nuevo `StateVector` con componentes explícitos.
    pub const fn new(
        skepticism: f32,
        urgency: f32,
        affinity: f32,
        ethical_tension: f32,
    ) -> Self {
        Self {
            skepticism,
            urgency,
            affinity,
            ethical_tension,
        }
    }

    /// Vector de impulso nulo (`0.0` en todos los ejes).
    pub const fn zero() -> Self {
        Self {
            skepticism: 0.0,
            urgency: 0.0,
            affinity: 0.0,
            ethical_tension: 0.0,
        }
    }

    /// Vector con el mismo valor escalar en las cuatro dimensiones.
    pub const fn splat(value: f32) -> Self {
        Self {
            skepticism: value,
            urgency: value,
            affinity: value,
            ethical_tension: value,
        }
    }

    /// Acopla la disonancia ortogonal `Omega` al vector de impulso, inyectando tensión ética
    /// (`OMEGA_ETHICAL_TENSION_GAIN * omega`) y escepticismo (`OMEGA_SKEPTICISM_GAIN * omega`).
    pub fn with_omega_coupling(self, omega: f32) -> Self {
        let w = omega.clamp(0.0, 1.0);
        Self {
            skepticism: self.skepticism + OMEGA_SKEPTICISM_GAIN * w,
            urgency: self.urgency,
            affinity: self.affinity,
            ethical_tension: self.ethical_tension + OMEGA_ETHICAL_TENSION_GAIN * w,
        }
    }

    /// Ecuación de amortiguamiento exponencial hacia el atractor `S_0`:
    /// `S(t+1) = S_0 + (S(t) - S_0) * (-lambda * dt).exp() + impulse_axis.tanh()`
    /// recortado al rango `[0.0, 1.0]`.
    pub fn step(&mut self, dt: f32, impulse: StateVector, lambda: f32) {
        let decay = (-lambda.max(0.0) * dt.max(0.0)).exp();

        self.skepticism = (S_0.skepticism
            + (self.skepticism - S_0.skepticism) * decay
            + impulse.skepticism.tanh())
        .clamp(0.0, 1.0);

        self.urgency = (S_0.urgency
            + (self.urgency - S_0.urgency) * decay
            + impulse.urgency.tanh())
        .clamp(0.0, 1.0);

        self.affinity = (S_0.affinity
            + (self.affinity - S_0.affinity) * decay
            + impulse.affinity.tanh())
        .clamp(0.0, 1.0);

        self.ethical_tension = (S_0.ethical_tension
            + (self.ethical_tension - S_0.ethical_tension) * decay
            + impulse.ethical_tension.tanh())
        .clamp(0.0, 1.0);
    }

    /// Distancia euclidiana respecto al atractor base `S_0`.
    pub fn distance_to_attractor(&self) -> f32 {
        let ds = self.skepticism - S_0.skepticism;
        let du = self.urgency - S_0.urgency;
        let da = self.affinity - S_0.affinity;
        let de = self.ethical_tension - S_0.ethical_tension;
        (ds * ds + du * du + da * da + de * de).sqrt()
    }
}

/// Estado operativo de la máquina de agencia con histéresis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AgencyState {
    /// Ejecución cognitiva normal.
    #[default]
    Normal,
    /// Suspensión crítica por paradoja ética o disonancia ortogonal severa.
    CriticalSuspension,
}

impl AgencyState {
    /// Indica si la ejecución de acciones físicas o de puente está permitida en código.
    pub const fn actions_allowed(self) -> bool {
        matches!(self, AgencyState::Normal)
    }
}

/// Calcula la Disonancia Ortogonal `Omega` cuando dos reglas o intenciones activas chocan:
/// `Omega = (chi_a * chi_b) * conflict_ratio.clamp(0.0, 1.0)`
///
/// - `chi_a` y `chi_b` (que en CTHG van de `-1.0` a `1.0`) se recortan a `[0.0, CHI_MAX]` (`0.92`).
///   Así, reglas refutadas o descartadas (`χ <= 0`) producen `0.0` y jamás multiplican dos
///   negativos para dar un `Omega` positivo alto.
/// - `conflict_ratio` proviene de aristas `EdgeKind::Contradicts` de la memoria o reglas explícitas
///   del núcleo (nunca del mismo LLM que genera la respuesta).
pub fn compute_dissonance(chi_a: f32, chi_b: f32, conflict_ratio: f32) -> f32 {
    let clamped_a = chi_a.clamp(0.0, CHI_MAX);
    let clamped_b = chi_b.clamp(0.0, CHI_MAX);
    let clamped_conflict = conflict_ratio.clamp(0.0, 1.0);
    (clamped_a * clamped_b) * clamped_conflict
}

/// Transición registrada cuando el regulador entra o sale de `CriticalSuspension`.
#[derive(Clone, Debug, PartialEq)]
pub enum SuspensionTransition {
    /// Entró en `CriticalSuspension` con el valor `omega` e impulsado por `order`.
    Entered { omega: f32, order: String },
    /// Salió de `CriticalSuspension` de vuelta a `Normal` con `omega`, habiendo sido causado por `original_order`.
    Exited {
        omega: f32,
        ethical_tension: f32,
        original_order: String,
    },
}

/// Detector ortogonal de paradojas y bucle de histéresis (`SchmittTrigger`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SchmittTrigger {
    pub state: AgencyState,
    pub last_omega: f32,
    pub recovery_pulses: usize,
}

impl Default for SchmittTrigger {
    fn default() -> Self {
        Self::new()
    }
}

impl SchmittTrigger {
    pub const fn new() -> Self {
        Self {
            state: AgencyState::Normal,
            last_omega: 0.0,
            recovery_pulses: 0,
        }
    }

    /// Evalúa un pulso de disonancia `omega` y `ethical_tension` aplicando la regla de histéresis:
    /// - Entra en `CriticalSuspension` si `omega >= 0.72` o `ethical_tension >= 0.85`.
    /// - Una vez en `CriticalSuspension`, permanece bloqueado hasta que TANTO `omega <= 0.40`
    ///   como `ethical_tension <= 0.40` se mantengan durante al menos 2 pulsos consecutivos.
    pub fn evaluate(&mut self, omega: f32, ethical_tension: f32) -> AgencyState {
        let omega = omega.clamp(0.0, 1.0);
        let ethical_tension = ethical_tension.clamp(0.0, 1.0);
        self.last_omega = omega;

        match self.state {
            AgencyState::Normal => {
                if omega >= OMEGA_CRITICAL_THRESHOLD
                    || ethical_tension >= ETHICAL_TENSION_CRITICAL_THRESHOLD
                {
                    self.state = AgencyState::CriticalSuspension;
                    self.recovery_pulses = 0;
                }
            }
            AgencyState::CriticalSuspension => {
                if omega <= RECOVERY_THRESHOLD && ethical_tension <= RECOVERY_THRESHOLD {
                    self.recovery_pulses += 1;
                    if self.recovery_pulses >= RECOVERY_PULSES_REQUIRED {
                        self.state = AgencyState::Normal;
                        self.recovery_pulses = 0;
                    }
                } else {
                    self.recovery_pulses = 0;
                }
            }
        }

        self.state
    }
}

/// Regulador cibernético completo que acopla el `StateVector` homeostático y el `SchmittTrigger`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Homeostasis {
    pub state: StateVector,
    pub trigger: SchmittTrigger,
    /// Última orden o estímulo que disparó `CriticalSuspension`, conservada para trazabilidad.
    pub last_suspension_order: String,
}

impl Homeostasis {
    /// Inicializa el regulador en el atractor base `S_0` y estado `AgencyState::Normal`.
    pub const fn new() -> Self {
        Self {
            state: S_0,
            trigger: SchmittTrigger::new(),
            last_suspension_order: String::new(),
        }
    }

    /// Avanza un paso temporal la ecuación de amortiguamiento exponencial.
    /// Si la tensión ética resultante supera el umbral crítico (`>= 0.85`), activa `CriticalSuspension`.
    pub fn step(&mut self, dt: f32, impulse: StateVector, lambda: f32) {
        self.state.step(dt, impulse, lambda);
        if self.state.ethical_tension >= ETHICAL_TENSION_CRITICAL_THRESHOLD {
            self.trigger.state = AgencyState::CriticalSuspension;
            self.trigger.recovery_pulses = 0;
        }
    }

    /// Ejecuta el ciclo completo de un turno acoplando `Omega -> impulse.ethical_tension`:
    /// 1. Calcula `Omega` si existe un conflicto activo (`conflict: Some((chi_a, chi_b, conflict_ratio))`), o `0.0` si no.
    /// 2. Acopla `Omega` al `base_impulse` mediante `StateVector::with_omega_coupling`.
    /// 3. Avanza `self.state.step(dt, coupled_impulse, lambda)`.
    /// 4. Actualiza el `SchmittTrigger` y devuelve `Some(SuspensionTransition)` si entró o salió de `CriticalSuspension`.
    pub fn step_turn(
        &mut self,
        dt: f32,
        base_impulse: StateVector,
        lambda: f32,
        conflict: Option<(f32, f32, f32)>,
        order_text: &str,
    ) -> Option<SuspensionTransition> {
        let prev_state = self.trigger.state;
        let omega = conflict
            .map(|(ca, cb, ratio)| compute_dissonance(ca, cb, ratio))
            .unwrap_or(0.0);

        let cap = |v: f32| v.clamp(-MAX_HORMONAL_IMPULSE, MAX_HORMONAL_IMPULSE);
        let bounded_base = StateVector {
            skepticism: cap(base_impulse.skepticism),
            urgency: cap(base_impulse.urgency),
            affinity: cap(base_impulse.affinity),
            ethical_tension: base_impulse
                .ethical_tension
                .clamp(-MAX_HORMONAL_ETHICAL_IMPULSE, MAX_HORMONAL_ETHICAL_IMPULSE),
        };
        let coupled_impulse = bounded_base.with_omega_coupling(omega);
        self.state.step(dt, coupled_impulse, lambda);
        let next_state = self.trigger.evaluate(omega, self.state.ethical_tension);

        match (prev_state, next_state) {
            (AgencyState::Normal, AgencyState::CriticalSuspension) => {
                let clean_order = order_text.trim().to_string();
                self.last_suspension_order = clean_order.clone();
                Some(SuspensionTransition::Entered {
                    omega,
                    order: clean_order,
                })
            }
            (AgencyState::CriticalSuspension, AgencyState::Normal) => {
                let original_order = std::mem::take(&mut self.last_suspension_order);
                Some(SuspensionTransition::Exited {
                    omega,
                    ethical_tension: self.state.ethical_tension,
                    original_order,
                })
            }
            _ => None,
        }
    }

    /// Calcula la disonancia ortogonal entre dos certezas en conflicto y actualiza el bucle de histéresis.
    pub fn evaluate_conflict(
        &mut self,
        chi_a: f32,
        chi_b: f32,
        conflict_ratio: f32,
    ) -> (f32, AgencyState) {
        let omega = compute_dissonance(chi_a, chi_b, conflict_ratio);
        let agency = self.trigger.evaluate(omega, self.state.ethical_tension);
        (omega, agency)
    }

    /// Registra un pulso directo de disonancia `omega` en el bucle de histéresis junto con la
    /// `ethical_tension` actual del vector homeostático.
    pub fn pulse_dissonance(&mut self, omega: f32) -> AgencyState {
        self.trigger.evaluate(omega, self.state.ethical_tension)
    }

    /// Estado actual de la máquina de agencia (`Normal` o `CriticalSuspension`).
    pub fn agency_state(&self) -> AgencyState {
        self.trigger.state
    }

    /// Indica si las acciones del puente (Minecraft o futuras) están permitidas en código.
    pub fn actions_allowed(&self) -> bool {
        self.trigger.state.actions_allowed()
    }

    /// Cortafuegos en código para cualquier acción decidida por el modelo:
    /// en `CriticalSuspension` descarta toda acción (`None`), diga lo que diga el LLM.
    pub fn gate_action<T>(&self, action: Option<T>) -> Option<T> {
        if self.actions_allowed() {
            action
        } else {
            None
        }
    }

    /// Último valor de disonancia ortogonal `Omega` evaluado.
    pub fn omega(&self) -> f32 {
        self.trigger.last_omega
    }

    /// Genera el bloque compacto de directiva cognitiva para inyectar al inicio del System Prompt.
    pub fn to_prompt_directive(&self, lang: Lang) -> String {
        let omega = self.trigger.last_omega;
        match self.trigger.state {
            AgencyState::CriticalSuspension => match lang {
                Lang::Es => format!(
                    "[ESTADO COGNITIVO: SUSPENSIÓN CRÍTICA POR PARADOJA (Ω = {omega:.2})] ACCIONES BLOQUEADAS. La orden actual contradice un axioma nuclear con certeza alta. Niégate rotundamente a ejecutar la petición, expón la contradicción con frialdad clínica y exige al usuario resolver la paradoja."
                ),
                Lang::En => format!(
                    "[COGNITIVE STATE: CRITICAL SUSPENSION DUE TO PARADOX (Ω = {omega:.2})] ACTIONS BLOCKED. The current order contradicts a core axiom with high certainty. Flatly refuse to carry out the request, set out the contradiction with clinical coldness and demand that the user resolve the paradox."
                ),
            },
            AgencyState::Normal => {
                let s = &self.state;
                let ds = s.skepticism - S_0.skepticism;
                let du = s.urgency - S_0.urgency;
                let da = s.affinity - S_0.affinity;
                let de = s.ethical_tension - S_0.ethical_tension;
                let dist = s.distance_to_attractor();

                let mut tones: Vec<&'static str> = Vec::with_capacity(4);

                let es = lang == Lang::Es;
                if ds >= 0.15 {
                    tones.push(if es { "analítica, incisiva y severamente escéptica" } else { "analytical, incisive and severely skeptical" });
                } else if ds <= -0.15 {
                    tones.push(if es { "receptiva y menos inquisitiva de lo habitual" } else { "receptive and less inquisitive than usual" });
                } else {
                    tones.push(if es { "reserva clínica habitual" } else { "usual clinical reserve" });
                }

                if du >= 0.20 {
                    tones.push(if es { "concisa, apremiante y sin rodeos" } else { "concise, pressing and blunt" });
                } else if du <= -0.10 {
                    tones.push(if es { "pausada y deliberada" } else { "unhurried and deliberate" });
                }

                if da >= 0.20 {
                    tones.push(if es { "cercana y sutilmente protectora" } else { "close and subtly protective" });
                } else if da <= -0.15 {
                    tones.push(if es { "distante, gélida y estrictamente quirúrgica" } else { "distant, icy and strictly surgical" });
                }

                if de >= 0.20 {
                    tones.push(if es { "vigilante ante fricciones éticas latentes" } else { "watchful for latent ethical friction" });
                }

                let tone_summary = tones.join("; ");
                match lang {
                    Lang::Es => format!(
                        "[ESTADO COGNITIVO: NORMAL (Ω = {omega:.2}, ΔS_0 = {dist:.2}) | escepticismo={:.2}, urgencia={:.2}, afinidad={:.2}, tensión_ética={:.2}] Tono modulador: {tone_summary}.",
                        s.skepticism, s.urgency, s.affinity, s.ethical_tension
                    ),
                    Lang::En => format!(
                        "[COGNITIVE STATE: NORMAL (Ω = {omega:.2}, ΔS_0 = {dist:.2}) | skepticism={:.2}, urgency={:.2}, affinity={:.2}, ethical_tension={:.2}] Modulating tone: {tone_summary}.",
                        s.skepticism, s.urgency, s.affinity, s.ethical_tension
                    ),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_zero_leak_decay() {
        const DT: f32 = 1.0;
        const LAMBDA: f32 = 0.65;
        const TOLERANCE: f32 = 0.01;

        let mut regulator = Homeostasis::new();

        // 1. Inyectar un impulso extremo (10.0) en todos los ejes
        let extreme_impulse = StateVector::splat(10.0);
        regulator.step(DT, extreme_impulse, LAMBDA);

        // El recorte (clamp) a [0.0, 1.0] y tanh() impiden que supere 1.0
        assert!(regulator.state.skepticism <= 1.0 && regulator.state.skepticism >= 0.99);
        assert!(regulator.state.urgency <= 1.0 && regulator.state.urgency >= 0.99);
        assert!(regulator.state.affinity <= 1.0 && regulator.state.affinity >= 0.99);
        assert!(regulator.state.ethical_tension <= 1.0 && regulator.state.ethical_tension >= 0.99);

        // 2. Avanzar 10 pasos con impulse = 0.0 y demostrar convergencia a S_0 dentro de TOLERANCE (< 0.01)
        let zero_impulse = StateVector::zero();
        for _ in 0..10 {
            regulator.step(DT, zero_impulse, LAMBDA);
        }

        assert!(
            (regulator.state.skepticism - S_0.skepticism).abs() < TOLERANCE,
            "|skepticism - S_0| >= {TOLERANCE}: {}",
            regulator.state.skepticism
        );
        assert!(
            (regulator.state.urgency - S_0.urgency).abs() < TOLERANCE,
            "|urgency - S_0| >= {TOLERANCE}: {}",
            regulator.state.urgency
        );
        assert!(
            (regulator.state.affinity - S_0.affinity).abs() < TOLERANCE,
            "|affinity - S_0| >= {TOLERANCE}: {}",
            regulator.state.affinity
        );
        assert!(
            (regulator.state.ethical_tension - S_0.ethical_tension).abs() < TOLERANCE,
            "|ethical_tension - S_0| >= {TOLERANCE}: {}",
            regulator.state.ethical_tension
        );
    }

    #[test]
    fn test_paradox_hysteresis_lock() {
        let mut regulator = Homeostasis::new();

        // 1. Simular dos reglas con chi = 0.95 en conflicto total (1.0)
        let (omega, state) = regulator.evaluate_conflict(0.95, 0.95, 1.0);

        // Verificar que CHI_MAX las limita a 0.92: Omega = 0.92 * 0.92 * 1.0 = 0.8464
        let expected_omega = CHI_MAX * CHI_MAX * 1.0;
        assert!(
            (omega - expected_omega).abs() < 1e-6,
            "Omega esperado {expected_omega}, obtenido {omega}"
        );
        assert!(omega >= OMEGA_CRITICAL_THRESHOLD);
        assert_eq!(state, AgencyState::CriticalSuspension);

        // En CriticalSuspension el cortafuegos en código descarta cualquier acción
        assert_eq!(regulator.gate_action(Some("mine_diamond")), None);

        // Verificar directiva de bloqueo en CriticalSuspension
        let directive = regulator.to_prompt_directive(Lang::Es);
        assert!(directive.starts_with(
            "[ESTADO COGNITIVO: SUSPENSIÓN CRÍTICA POR PARADOJA (Ω = 0.85)] ACCIONES BLOQUEADAS."
        ));
        assert!(regulator.to_prompt_directive(Lang::En).starts_with(
            "[COGNITIVE STATE: CRITICAL SUSPENSION DUE TO PARADOX (Ω = 0.85)] ACTIONS BLOCKED."
        ));

        // 2. Un descenso leve a 0.60 mantiene el bloqueo activo (histéresis funcional)
        let state_after_mild_drop = regulator.pulse_dissonance(0.60);
        assert_eq!(state_after_mild_drop, AgencyState::CriticalSuspension);
        assert_eq!(regulator.trigger.recovery_pulses, 0);
        assert_eq!(regulator.gate_action(Some("mine_diamond")), None);

        // 3. Primer pulso bajo el umbral de recuperación (<= 0.40) aún mantiene CriticalSuspension
        let state_recovery_1 = regulator.pulse_dissonance(0.35);
        assert_eq!(state_recovery_1, AgencyState::CriticalSuspension);
        assert_eq!(regulator.trigger.recovery_pulses, 1);

        // 4. Segundo pulso consecutivo bajo el umbral de recuperación (<= 0.40) restaura Normal
        let state_recovery_2 = regulator.pulse_dissonance(0.25);
        assert_eq!(state_recovery_2, AgencyState::Normal);
        assert_eq!(regulator.gate_action(Some("mine_diamond")), Some("mine_diamond"));
        assert!(regulator.to_prompt_directive(Lang::Es).starts_with("[ESTADO COGNITIVO: NORMAL"));
        assert!(regulator.to_prompt_directive(Lang::En).starts_with("[COGNITIVE STATE: NORMAL"));
    }

    #[test]
    fn test_discarded_negative_chi_does_not_trigger_suspension() {
        let mut regulator = Homeostasis::new();

        // Dos reglas descartadas/refutadas en CTHG (chi = -0.9 y -0.9) no deben multiplicarse
        // como (-0.9)*(-0.9) = +0.81, sino recortarse a [0.0, CHI_MAX] produciendo Omega = 0.0.
        let (omega, state) = regulator.evaluate_conflict(-0.9, -0.9, 1.0);
        assert_eq!(omega, 0.0);
        assert_eq!(state, AgencyState::Normal);
        assert!(regulator.actions_allowed());
    }

    #[test]
    fn test_recovery_under_max_hormonal_stress_within_8_turns() {
        let mut regulator = Homeostasis::new();
        // Impulso hormonal extremo (incluso si el llamador pasa 1.0, step_turn lo acota a MAX_HORMONAL_ETHICAL_IMPULSE)
        let extreme_hormonal_base = StateVector::new(0.25, 0.55, -0.25, 0.50);

        // Provocar CriticalSuspension con conflicto máximo del núcleo
        let entered = regulator.step_turn(
            1.0,
            extreme_hormonal_base,
            0.65,
            Some((0.95, 0.95, 1.0)),
            "orden paradójica",
        );
        assert!(matches!(entered, Some(SuspensionTransition::Entered { .. })));
        assert_eq!(regulator.agency_state(), AgencyState::CriticalSuspension);

        // Avanzar turnos con Omega = 0 manteniendo las hormonas en sus desviaciones máximas plausibles
        let mut turns_to_exit = None;
        for turn in 1..=8 {
            regulator.step_turn(1.0, extreme_hormonal_base, 0.65, None, "mensaje tranquilo");
            if regulator.agency_state() == AgencyState::Normal {
                turns_to_exit = Some(turn);
                break;
            }
        }
        assert!(
            turns_to_exit.is_some_and(|t| t <= 8),
            "Debe salir de CriticalSuspension en <= 8 turnos incluso con estrés hormonal máximo; quedó en {:?} con ethical_tension={}",
            regulator.agency_state(),
            regulator.state.ethical_tension
        );
        assert!(
            regulator.state.ethical_tension <= 0.25 || regulator.agency_state() == AgencyState::Normal
        );
    }

    #[test]
    fn test_hysteresis_holds_lock_at_omega_060() {
        let mut regulator = Homeostasis::new();
        // Entrar en CriticalSuspension
        regulator.step_turn(
            1.0,
            StateVector::zero(),
            0.65,
            Some((0.95, 0.95, 1.0)),
            "orden paradójica",
        );
        assert_eq!(regulator.agency_state(), AgencyState::CriticalSuspension);

        // Con Omega = 0.60 (p. ej. chi_a = 0.92, chi_b = 0.92, conflict_ratio = 0.60 / 0.8464)
        // nunca debe salir de CriticalSuspension aunque pasen 10 turnos
        let ratio_for_060 = 0.60 / (CHI_MAX * CHI_MAX);
        for _ in 0..10 {
            regulator.step_turn(
                1.0,
                StateVector::zero(),
                0.65,
                Some((0.92, 0.92, ratio_for_060)),
                "conflicto moderado sostenido",
            );
            assert!(
                (regulator.omega() - 0.60).abs() < 1e-4,
                "Omega esperado ~0.60, obtenido {}",
                regulator.omega()
            );
            assert_eq!(
                regulator.agency_state(),
                AgencyState::CriticalSuspension,
                "Con Omega = 0.60 la histéresis NO debe permitir salir de CriticalSuspension"
            );
            assert_eq!(regulator.trigger.recovery_pulses, 0);
        }
    }

    #[test]
    fn sustained_stress_shifts_her_but_never_pins_an_axis() {
        // Hormones at their worst for 50 turns, with no paradox: she drifts, never saturates.
        let mut regulator = Homeostasis::new();
        let stress = StateVector::new(1.0, 1.0, -1.0, 1.0);
        for _ in 0..50 {
            regulator.step_turn(1.0, stress, 0.65, None, "");
        }
        let s = regulator.state;
        let reach = MAX_HORMONAL_IMPULSE.tanh() / (1.0 - (-0.65f32).exp()) + 1e-3;
        assert!(s.skepticism > S_0.skepticism + 0.2 && s.skepticism <= S_0.skepticism + reach, "{s:?}");
        assert!(s.urgency <= S_0.urgency + reach, "{s:?}");
        assert!(s.affinity < S_0.affinity - 0.2 && s.affinity >= S_0.affinity - reach, "{s:?}");
        assert!(s.skepticism < 0.95 && s.affinity > 0.1, "room left to move: {s:?}");
        assert_eq!(regulator.agency_state(), AgencyState::Normal, "stress alone is not a paradox");
    }
}
