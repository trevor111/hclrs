use ast::{Assignment, Statement, ConstDecl, WireDecl, WireWidth, WireValue, WireValues, Expr};
use parser::parse_Statements;
use extprim::u128::u128;
use errors::Error;
use lexer::Lexer;
use std::collections::hash_set::HashSet;
use std::collections::hash_map::HashMap;
use std::collections::btree_map::BTreeMap;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::hash::Hash;
use std::io::Write;

struct Graph<T> {
    edges: HashMap<T, HashSet<T>>,
    edges_inverted: HashMap<T, HashSet<T>>,
    nodes: HashSet<T>,
    num_edges: usize,
}

impl<T: Eq + Hash + Clone + Debug> Graph<T> {
    fn add_node(&mut self, node: T) {
        self.nodes.insert(node);
    }

    fn insert(&mut self, from: T, to: T) {
        debug!("insert node {:?} / {:?}", from, to);
        self.add_node(from.clone());
        self.add_node(to.clone());
        let for_node = self.edges.entry(from.clone()).or_insert_with(|| HashSet::new());
        (*for_node).insert(to.clone());
        let inverted_for_node = self.edges_inverted.entry(to).or_insert_with(|| HashSet::new());
        (*inverted_for_node).insert(from);
        self.num_edges = self.num_edges + 1;
    }

    fn contains_node(&self, node: &T) -> bool {
        return self.nodes.contains(&node);
    }

    fn contains(&self, from: T, to: T) -> bool {
        if let Some(ref out_edges) = self.edges.get(&from) {
            out_edges.contains(&to)
        } else {
            false
        }
    }

    // topological sort, or find cycle
    fn topological_sort(&self) -> Result<Vec<T>, Vec<T>> {
        let mut result = Vec::new();

        // Step 1: Queue nodes with no incoming edges
        let mut queue = VecDeque::new();
        let mut num_in_unvisited = HashMap::new();
        for node in &self.nodes {
            if !self.edges_inverted.contains_key(&node) {
                debug!("found starting node {:?}", node);
                queue.push_front(node.clone());
            }
            num_in_unvisited.insert(node.clone(),
                self.edges_inverted.get(&node).map_or(0, |x| x.len())
            );
        }

        debug!("Initial queue: {:?}", queue);
        debug!("Initial in counts: {:?}", num_in_unvisited);

        let mut visited = HashSet::new();
        while let Some(cur) = queue.pop_back() {
            debug!("Process {:?} from queue", cur);
            result.push(cur.clone());
            if let Some(out_edges) = self.edges.get(&cur) {
                for out_node in out_edges {
                    let pair = (cur.clone(), out_node.clone());
                    debug!("Found edge {:?}", pair);
                    if !visited.contains(&pair) {
                        visited.insert(pair);
                        let new_in_unvisited = num_in_unvisited.get(out_node).unwrap_or(&0) - 1;
                        num_in_unvisited.insert(out_node.clone(), new_in_unvisited);
                        if new_in_unvisited == 0 {
                            debug!("Now enqueue {:?}", out_node);
                            queue.push_front(out_node.clone());
                        }
                    }
                }
            }
        }

        if visited.len() != self.num_edges {
            // found a cycle
            unimplemented!();
        }

        Ok(result)
    }

    #[cfg(test)]
    fn in_edges(&self, to: &T) -> HashSet<T> {
        if let Some(result) = self.edges_inverted.get(to) {
            result.clone()
        } else {
            HashSet::new()
        }
    }

    #[cfg(test)]
    fn out_edges(&self, to: &T) -> HashSet<T> {
        if let Some(result) = self.edges.get(to) {
            result.clone()
        } else {
            HashSet::new()
        }
    }

    fn new() -> Graph<T> {
        Graph {
            edges: HashMap::new(),
            edges_inverted: HashMap::new(),
            nodes: HashSet::new(),
            num_edges: 0,
        }
    }
}

#[cfg(test)]
fn verify_sort<T: Eq + Clone + Hash + Debug>(graph: &Graph<T>) {
    if let Ok(the_result) = graph.topological_sort() {
        let mut seen = HashSet::new();
        for node in &the_result {
            for other in graph.out_edges(&node) {
                assert!(!seen.contains(&other), "{:?} -> {:?} violates order {:?}",
                    node, other, the_result);
            }
            seen.insert(node.clone());
        }
    } else {
        assert!(false);
    }
}


#[test]
fn test_graph() {
    let mut graph = Graph::new();
    graph.insert("foo", "bar");
    graph.insert("bar", "baz");
    verify_sort(&graph);
    graph.insert("foo", "baz");
    verify_sort(&graph);
    graph.insert("foo", "quux");
    graph.insert("quux", "other");
    verify_sort(&graph);
    graph.add_node("unused");
    verify_sort(&graph);
}

#[derive(Debug,PartialEq,Eq,Clone)]
pub enum Action {
    // not included:
        // register bank processing (done at beginning of cycle)
    Assign(String, Box<Expr>, WireWidth),
    ReadProgramRegister { number: String, out_port: String },
    ReadMemory { is_read: Option<String>, address: String, out_port: String, bytes: u8 },
    // these actions MUST be done last:
    WriteProgramRegister { number: String, in_port: String },
    WriteMemory { is_write: Option<String>, address: String, in_port: String, bytes: u8 },
    SetStatus { in_wire: String },
}

// psuedo-assignment for fixed functionality
pub struct FixedFunction {
    in_wires: Vec<WireDecl>,
    out_wire: Option<String>,
    action: Action,
    mandatory: bool,
    is_last: bool,
}

impl FixedFunction {
    fn read_port(number: &str, output: &str) -> FixedFunction {
        FixedFunction {
            in_wires: vec!(WireDecl { name: String::from(number), width: WireWidth::Bits(4) }),
            out_wire: Some(String::from(output)),
            action: Action::ReadProgramRegister {
                number: String::from(number),
                out_port: String::from(output),
            },
            mandatory: false,
            is_last: false,
        }
    }

    fn write_port(number: &str, input: &str) -> FixedFunction {
        FixedFunction {
            in_wires: vec!(
                WireDecl {
                    name: String::from(number),
                    width: WireWidth::Bits(4),
                },
                WireDecl {
                    name: String::from(input),
                    width: WireWidth::Bits(64),
                }
            ),
            out_wire: None,
            action: Action::WriteProgramRegister {
                number: String::from(number),
                in_port: String::from(input),
            },
            mandatory: false,
            is_last: true,
        }
    }
}

pub fn y86_fixed_functions() -> Vec<FixedFunction> {
    return vec!(
        FixedFunction {
            in_wires: vec!(WireDecl {
                name: String::from("Stat"),
                width: WireWidth::Bits(3),
            }),
            out_wire: None,
            action: Action::SetStatus { in_wire: String::from("Stat") },
            mandatory: true,
            is_last: true,
        },
        FixedFunction {
            in_wires: vec!(WireDecl {
                name: String::from("pc"),
                width: WireWidth::Bits(64),
            }),
            out_wire: Some(String::from("i10bytes")),
            action: Action::ReadMemory {
                is_read: None,
                address: String::from("pc"),
                out_port: String::from("i10bytes"),
                bytes: 10
            },
            mandatory: true,
            is_last: false,
        },
        FixedFunction {
            in_wires: vec!(WireDecl {
                name: String::from("mem_addr"),
                width: WireWidth::Bits(64),
            },
            WireDecl {
                name: String::from("mem_read"),
                width: WireWidth::Bits(1),
            }),
            out_wire: Some(String::from("mem_output")),
            action: Action::ReadMemory {
                is_read: Some(String::from("mem_read")),
                address: String::from("mem_addr"),
                out_port: String::from("mem_output"),
                bytes: 8
            },
            mandatory: false,
            is_last: false,
        },
        FixedFunction {
            // FIXME: some way to indicate that mem_write -> mem_input + mem_addr?
            in_wires: vec!(
                WireDecl {
                    name: String::from("mem_addr"),
                    width: WireWidth::Bits(64),
                },
                WireDecl {
                    name: String::from("mem_input"),
                    width: WireWidth::Bits(64),
                },
                WireDecl {
                    name: String::from("mem_write"),
                    width: WireWidth::Bits(1),
                }
            ),
            out_wire: None,
            action: Action::WriteMemory {
                is_write: Some(String::from("mem_write")),
                address: String::from("mem_addr"),
                in_port: String::from("mem_input"),
                bytes: 8,
            },
            mandatory: false,
            is_last: false,
        },
        FixedFunction::read_port("reg_srcA", "reg_outputA"),
        FixedFunction::read_port("reg_srcB", "reg_outputB"),
        FixedFunction::write_port("reg_dstE", "reg_inputE"),
        FixedFunction::write_port("reg_dstM", "reg_inputM")
    )
}

#[derive(Debug)]
pub struct RegisterBank {
    label: String,
    signals: Vec<(String, String, WireWidth)>, // in, out
    defaults: WireValues, // mapped to out names
    stall_signal: String,
    bubble_signal: String,
}

pub struct RegisterWritePort {
    number: String,
    value: String
}

// interpreter representation of a program
#[derive(Debug)]
pub struct Program {
    constants: WireValues,
    actions: Vec<Action>,  // in topological order
    register_banks: Vec<RegisterBank>,
}


fn resolve_constants(exprs: &HashMap<&str, &Expr>) -> Result<HashMap<String, WireValue>, Error> {
    let mut graph = Graph::new();
    for (name, expr) in exprs {
        for in_name in expr.referenced_wires() {
            graph.insert(in_name, name);
        }
        graph.add_node(name);
    }
    if let Ok(sorted) = graph.topological_sort() {
        let mut results = HashMap::new();
        for name in sorted {
            let value = try!(exprs.get(&name).unwrap().evaluate(&results));
            results.insert(
                String::from(name),
                value
            );
        }
        Ok(results)
    } else {
        unimplemented!();
    }
}

fn assignments_to_actions<'a>(
        assignments: &'a HashMap<&'a str, &Expr>,
        widths: &'a HashMap<&'a str, WireWidth>,
        known_values: &'a HashSet<&'a str>,
        fixed_functions: &'a Vec<FixedFunction>,
    ) -> Result<Vec<Action>, Error> {
    let mut fixed_by_output = HashMap::new();
    let mut fixed_no_output = Vec::new();
    let mut graph = Graph::new();
    for (name, expr) in assignments {
        graph.add_node(*name);
        for in_name in expr.referenced_wires() {
            if !known_values.contains(in_name) {
                if !assignments.contains_key(in_name) {
                    return Err(Error::UndefinedWire(String::from(in_name)));
                }
                graph.insert(in_name, name);
            }
        }
    }

    let mut unused_fixed_inputs = HashSet::new();
    let mut used_fixed_inputs = HashSet::new();
    for fixed in fixed_functions.iter() {
        let mut missing_inputs: Vec<&str> = Vec::new();
        for in_name in &fixed.in_wires {
            if known_values.contains(in_name.name.as_str()) {
                return Err(Error::RedefinedBuiltinWire(in_name.name.clone()));
            }
            if !assignments.contains_key(in_name.name.as_str()) {
                missing_inputs.push(in_name.name.as_str());
            }
        }
        if fixed.mandatory && missing_inputs.len() > 0 {
            let missing_list = missing_inputs.iter().map(|x| Error::UnsetWire(String::from(*x))).collect();
            return Err(Error::MultipleErrors(missing_list));
        } else if missing_inputs.len() > 0 {
            if let Some(ref name) = fixed.out_wire {
                if graph.contains_node(&name.as_str()) {
                    let missing_list = missing_inputs.iter().map(|x| Error::UnsetWire(String::from(*x))).collect();
                    return Err(Error::MultipleErrors(missing_list));
                }
            }
            if missing_inputs.len() != fixed.in_wires.len() {
                for in_name in &fixed.in_wires {
                    if assignments.contains_key(in_name.name.as_str()) {
                        unused_fixed_inputs.insert(in_name.name.as_str());
                    }
                }
            }
            continue;
        }
        match fixed.out_wire {
            None => {
                fixed_no_output.push(fixed);
            },
            Some(ref name) => {
                if known_values.contains(name.as_str()) ||
                   assignments.contains_key(name.as_str()) {
                    return Err(Error::RedefinedBuiltinWire(name.clone()));
                }
                fixed_by_output.insert(name.as_str(), fixed);
                for in_name in &fixed.in_wires {
                    used_fixed_inputs.insert(in_name.name.as_str());
                    graph.insert(in_name.name.as_str(), name.as_str());
                }
            }
        }
    }

    // if any piece of fixed functionality was not created because some but not all of its
    // inputs exist, trigger an error unless all those inputs are used by other fixed
    // functionality.

    // this makes doing something like setting reg_dstE without setting reg_inputE an error,
    // but doesn't make something like setting mem_addr without
    {
        let mut missing_inputs = Vec::new();
        for name in unused_fixed_inputs {
            if !used_fixed_inputs.contains(name) {
                missing_inputs.push(name);
            }
        }

        if missing_inputs.len() > 0 {
            let missing_list = missing_inputs.iter().map(
                |x| Error::UnsetWire(String::from(*x))).collect();
            return Err(Error::MultipleErrors(missing_list));
        }
    }

    let mut result = Vec::new();

    if let Ok(sorted) = graph.topological_sort() {
        // FIXME: covered is just a sanity-check, should be removeable
        let mut covered = known_values.clone();
        for name in sorted {
            match assignments.get(name) {
                Some(expr) => {
                    for in_name in expr.referenced_wires() {
                        assert!(covered.contains(&in_name));
                    }
                    if let Some(the_width) = widths.get(name) {
                        // FIXME: allow implict wire length shrinking?
                        //        (check with HCL2D did)
                        expr.width(widths)?.combine_expr_and_wire(*the_width, name, expr)?;
                        result.push(Action::Assign(
                            String::from(name),
                            Box::new((*expr).clone()),
                            *the_width,
                        ));
                    } else {
                        return Err(Error::UndefinedWire(String::from(name)));
                    }
                },
                None => {
                    let fixed = fixed_by_output.get(name).unwrap();
                    for in_name in &fixed.in_wires {
                        assert!(covered.contains(in_name.name.as_str()));
                    }
                    result.push(fixed.action.clone());
                }
            }
            covered.insert(name);
        }
    } else {
        unimplemented!();
    }

    for fixed in &fixed_no_output {
        result.push(fixed.action.clone());
    }

    return Ok(result);
}

fn y86_preamble() -> Vec<Statement> {
    let preamble_text = "
const STAT_BUB = 0b000, STAT_AOK = 0b001, STAT_HLT = 0b010;  # expected behavior
const STAT_ADR = 0b011, STAT_INS = 0b100, STAT_PIP = 0b110;  # error conditions

const REG_RAX = 0b0000, REG_RCX = 0b0001, REG_RDX = 0b0010, REG_RBX = 0b0011;
const REG_RSP = 0b0100, REG_RBP = 0b0101, REG_RSI = 0b0110, REG_RDI = 0b0111;
const REG_R8  = 0b1000, REG_R9  = 0b1001, REG_R10 = 0b1010, REG_R11 = 0b1011;
const REG_R12 = 0b1100, REG_R13 = 0b1101, REG_R14 = 0b1110, REG_NONE= 0b1111;

# icodes; see figure 4.2
const HALT   = 0b0000, NOP    = 0b0001, RRMOVQ = 0b0010, IRMOVQ = 0b0011;
const RMMOVQ = 0b0100, MRMOVQ = 0b0101, OPQ    = 0b0110, JXX    = 0b0111;
const CALL   = 0b1000, RET    = 0b1001, PUSHQ  = 0b1010, POPQ   = 0b1011;
const CMOVXX = RRMOVQ;

# ifuns; see figure 4.3
const ALWAYS = 0b0000, LE   = 0b0001, LT   = 0b0010, EQ   = 0b0011;
const NE     = 0b0100, GE   = 0b0101, GT   = 0b0110;
const ADDQ   = 0b0000, SUBQ = 0b0001, ANDQ = 0b0010, XORQ = 0b0011;
";
    let mut errors = Vec::new();
    // FIXME: adjust locations somehow
    let result = parse_Statements(&mut errors, Lexer::new(preamble_text)).unwrap();
    assert_eq!(errors.len(), 0);
    result
}

impl Program {
    pub fn new_y86(statements: Vec<Statement>) -> Result<Program, Error> {
        let mut statements = statements;
        statements.extend(y86_preamble());
        Program::new(statements, y86_fixed_functions())
    }

    pub fn new(
        statements: Vec<Statement>,
        fixed_functions: Vec<FixedFunction>
        // TODO: preamble (constants)
    ) -> Result<Program, Error> {
        // Step 1: Split statements into constant declarations, wire declarations, assignments
        let mut constants_raw: HashMap<&str, &Expr> = HashMap::new();
        let mut wires = HashMap::new();
        let mut needed_wires = HashSet::new();
        let mut assignments = HashMap::new();
        let mut register_banks_raw = Vec::new();
        for fixed in &fixed_functions {
            for ref in_wire in &fixed.in_wires {
                wires.insert(in_wire.name.as_str(), in_wire.width);
            }
        }
        // FIXME: detect duplicates somewhere here
        for statement in &statements {
            match *statement {
                Statement::ConstDecls(ref decls) => {
                    for ref decl in decls {
                        constants_raw.insert(decl.name.as_str(), &*decl.value);
                    }
                },
                Statement::WireDecls(ref decls) => {
                    for ref decl in decls {
                        wires.insert(decl.name.as_str(), decl.width);
                        needed_wires.insert(decl.name.as_str());
                    }
                },
                Statement::Assignment(ref assign) => {
                    for name in &assign.names {
                        // FIXME: detect multiple declarations
                        assignments.insert(name.as_str(), &*assign.value);
                    }
                },
                Statement::RegisterBankDecl(ref decl) => {
                    register_banks_raw.push(decl);
                }
                _ => unimplemented!(),
            }
        }

        debug!("const decls: {:?}", constants_raw);
        debug!("wire decls: {:?}", wires);
        debug!("assignments: {:?}", assignments);

        // Step 2: find constants values
        let constants = try!(resolve_constants(&constants_raw));

        // Step 3: resolve register banks
        let mut register_banks = Vec::new();
        for decl in &register_banks_raw {
            // FIXME: should really iterate over graphemes
            let name_chars: Vec<char> = decl.name.chars().collect();
            if name_chars.len() != 2 {
                return Err(Error::InvalidRegisterBankName(decl.name.clone()));
            }
            let in_prefix = name_chars[0];
            let out_prefix = name_chars[1];
            if !in_prefix.is_lowercase() || !out_prefix.is_uppercase() {
                return Err(Error::InvalidRegisterBankName(decl.name.clone()));
            }
            let mut signals = Vec::new();
            let mut defaults = HashMap::new();
            let mut stall_signal = String::from("stall_");
            stall_signal.push(out_prefix);
            let mut bubble_signal = String::from("bubble_");
            bubble_signal.push(out_prefix);
            for register in &decl.registers {
                let mut in_name = String::new();
                let mut out_name = String::new();
                in_name.push(in_prefix);
                out_name.push(out_prefix);
                in_name.push('_');
                out_name.push('_');
                in_name.push_str(register.name.as_str());
                out_name.push_str(register.name.as_str());
                // FIXME: better errors if failure here
                let value = register.default.evaluate(&constants)?;
                // FIXME: better error
                if None == value.width.combine(register.width) {
                    // FIXME: accumulate errors?
                    return Err(Error::MismatchedRegisterDefaultWidths(
                        decl.name.clone(),
                        register.name.clone(),
                        (*register.default).clone(),
                    ));

                }
                defaults.insert(out_name.clone(), value.as_width(register.width));
                debug!("Generated wires {} and {} for register", in_name, out_name);
                signals.push((in_name, out_name, register.width));
            }
            // FIXME: detect redefinition of signals
            register_banks.push(RegisterBank {
                label: decl.name.clone(),
                signals: signals,
                defaults: defaults,
                stall_signal: stall_signal,
                bubble_signal: bubble_signal,
            });
        }

        let actions = {
            // create nonmutable reference so we can borrow strings from register banks
            let register_banks = &register_banks;
            // move wires and needed_wires so they are dropped before register_banks
            let mut wires = wires;
            let mut needed_wires = needed_wires;

            // track widths, values we do/don't need assignment statements for
            let mut known_values = HashSet::new();
            for bank in register_banks {
                for signal in &bank.signals {
                    let in_name = &signal.0;
                    let out_name = &signal.1;
                    let width = signal.2;
                    wires.insert(out_name.as_str(), width);
                    known_values.insert(out_name.as_str());
                    wires.insert(in_name.as_str(), width);
                    needed_wires.insert(in_name.as_str());
                }
                wires.insert(bank.stall_signal.as_str(), WireWidth::Bits(1));
                wires.insert(bank.bubble_signal.as_str(), WireWidth::Bits(1));
            }

            // Step 4: Check for missing wires
            for name in needed_wires {
                if !assignments.contains_key(name) {
                    return Err(Error::UnsetWire(String::from(name)));
                }
            }

            // Step 5: order remaining assignments
            for key in constants_raw.keys() {
                known_values.insert(*key);
                wires.insert(key, constants.get(&String::from(*key)).unwrap().width);
            }

            assignments_to_actions(&assignments, &wires,
                                   &known_values, &fixed_functions)?
        };

        Ok(Program {
            constants: constants,
            actions: actions,
            register_banks: register_banks,
        })
    }

    pub fn constants(&self) -> WireValues {
        self.constants.clone()
    }

    pub fn initial_state(&self) -> WireValues {
        let mut values = self.constants();
        for bank in &self.register_banks {
            for signal in &bank.signals {
                let in_name = &signal.0;
                let out_name = &signal.1;
                let the_value = *bank.defaults.get(out_name).unwrap();
                values.insert(in_name.clone(), the_value);
                values.insert(out_name.clone(), the_value);
            }
            values.insert(bank.bubble_signal.clone(), WireValue::false_value());
            values.insert(bank.stall_signal.clone(), WireValue::false_value());
        }
        values
    }

    fn process_register_banks(&self, values: &mut WireValues) -> Result<(), Error> {
        for bank in &self.register_banks {
            let stalled = values.get(&bank.stall_signal).unwrap().is_true();
            let bubbled = values.get(&bank.bubble_signal).unwrap().is_true();
            // FIXME: correct stall + bubble behavior
            if bubbled {
                debug!("bubble {}", bank.bubble_signal);
                for (k, v) in &bank.defaults {
                    *values.get_mut(k).unwrap() = *v;
                }
            } else if !stalled {
                for signal in &bank.signals {
                    let in_name = &signal.0;
                    let out_name = &signal.1;
                    debug!("copy {} -> {}", in_name, out_name);
                    let new_value = *values.get(in_name).unwrap();
                    *values.get_mut(out_name).unwrap() = new_value;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Memory {
    data: BTreeMap<u64, u8>
}

impl Memory {
    pub fn new() -> Memory {
        Memory { data: BTreeMap::new() }
    }

    pub fn read(&self, address: u64, bytes: u8) -> WireValue {
        assert!(bytes <= 16);
        let mut result = u128::new(0);
        let mut remaining = bytes;
        let total = remaining;
        let mut cur_addr = address;
        debug!("reading {:#x} ({:?} bytes)", address, bytes);
        while remaining > 0 {
            result |= u128::new(*self.data.get(&cur_addr).unwrap_or(&0) as u64) << ((total - remaining) * 8);
            debug!("reading {:#x}; accumulated result is {:#x}", cur_addr, result);
            cur_addr += 1;
            remaining -= 1;
        }
        WireValue { bits: result, width: WireWidth::Bits(bytes * 8) }
    }

    pub fn write(&mut self, address: u64, value: u128, bytes: u8) {
        assert!(bytes <= 16);
        let mut remaining = bytes;
        let total = remaining;
        let mut cur_addr = address;
        debug!("write {:#x} ({:?} bytes) into {:#x}", value, bytes, address);
        while remaining > 0 {
            let to_write = (value >> ((total - remaining) * 8)).low64() as u8;
            debug!("writing {:#x} into {:#x}", to_write, cur_addr);
            self.data.insert(cur_addr, to_write);
            cur_addr += 1;
            remaining -= 1;
        }
    }

    // FIXME: iteration for output
}

#[derive(Debug)]
pub struct RunningProgram {
    program: Program,
    cycle: u32,
    values: WireValues,
    memory: Memory,
    registers: Vec<u64>,
    zero_register: usize,
    last_status: Option<u8>,
    timeout: u32,
}

impl RunningProgram {
    pub fn new(program: Program,
               num_registers: usize,
               zero_register: usize) -> RunningProgram {
        let values = program.initial_state();
        let mut registers = Vec::new();
        for i in 0..num_registers {
            registers.push(0);
        }
        RunningProgram {
            program: program,
            cycle: 0,
            values: values,
            memory: Memory::new(),
            registers: registers,
            zero_register: zero_register,
            last_status: None,
            timeout: u32::max_value(),
        }
    }

    pub fn new_y86(program: Program) -> RunningProgram {
        RunningProgram::new(
            program,
            16,
            15
        )
    }

    fn step_in_place(&mut self) -> Result<(), Error> {
        self.program.process_register_banks(&mut self.values)?;
        for action in &self.program.actions {
            debug!("processing action {:?}", action);
            match action {
               &Action::Assign(ref name, ref expr, ref width) => {
                  let result = expr.evaluate(&self.values)?.as_width(*width);
                  debug!("computed value {:?}", result);
                  let mut inserted = false;
                  if let Some(value) = self.values.get_mut(name) {
                      *value = result;
                      inserted = true;
                  }
                  if !inserted {
                      self.values.insert(name.clone(), result);
                  }
               },
               &Action::ReadMemory { ref is_read, ref address, ref out_port, ref bytes } => {
                   let do_read = match is_read {
                       &None => true,
                       &Some(ref wire) => self.values.get(wire).unwrap().is_true(),
                   };
                   if do_read {
                       let address_value = *self.values.get(address).unwrap();
                       let value = self.memory.read(address_value.bits.low64(), *bytes);
                       self.values.insert(out_port.clone(), value);
                   } else {
                       // keep the result well-defined
                       let zero = WireValue::new(u128::new(0)).as_width(WireWidth::from((bytes * 8) as usize));
                       self.values.insert(out_port.clone(), zero);
                   }
               },
               &Action::WriteMemory { ref is_write, ref address, ref in_port, ref bytes } => {
                   let do_write = match is_write {
                       &None => true,
                       &Some(ref wire) => self.values.get(wire).unwrap().is_true(),
                   };
                   if do_write {
                       let address_value = self.values.get(address).unwrap();
                       let input_value = self.values.get(in_port).unwrap();
                       self.memory.write(address_value.bits.low64(), input_value.bits, *bytes);
                   }
               },
               &Action::SetStatus { ref in_wire } => {
                   self.last_status = Some(self.values.get(in_wire).unwrap().bits.low64() as u8);
               },
               &Action::ReadProgramRegister { ref number, ref out_port } => {
                   let number = self.values.get(number).unwrap().bits.low64() as usize;
                   if number < self.registers.len() {
                       self.values.insert(out_port.clone(),
                           WireValue { bits: u128::new(self.registers[number]), width: WireWidth::Bits(64) }
                       );
                   } else {
                       self.values.insert(out_port.clone(), WireValue {
                           bits: u128::new(0), width: WireWidth::Bits(64)
                       });
                   }
               },
               &Action::WriteProgramRegister { ref number, ref in_port } => {
                   let number = self.values.get(number).unwrap().bits.low64() as usize;
                   if number < self.registers.len() && number != self.zero_register {
                       self.registers[number] = self.values.get(in_port).unwrap().bits.low64();
                   }
               }
               _ => unimplemented!(),
            }
        }

        Ok(())
    }

    pub fn cycle(&self) -> u32 { self.cycle }

    pub fn values(&self) -> &WireValues { &self.values }

    pub fn step(&mut self) -> Result<(), Error> {
        try!(self.step_in_place());
        self.cycle += 1;
        Ok(())
    }

    // FIXME: hard-coded Y86 status codes
    pub fn done(&self) -> bool {
        self.values.get("Stat").unwrap_or(&WireValue::from_u64(1)).bits != u128::new(1) || self.cycle > self.timeout
    }

    pub fn halted(&self) -> bool {
        self.values.get("Stat").unwrap_or(&WireValue::from_u64(1)).bits == u128::new(2)
    }

    pub fn timed_out(&self) -> bool {
        self.cycle >= self.timeout
    }

    fn dump_program_registers_y86<W: Write>(&self, result: &mut W) {
        writeln!(result, "| RAX: {:16x}   RCX: {:16x}   RDX: {:16x} |",
            self.registers[0], self.registers[1], self.registers[2]);
        writeln!(result, "| RBX: {:16x}   RSP: {:16x}   RBP: {:16x} |",
            self.registers[3], self.registers[4], self.registers[5]);
        writeln!(result, "| RSI: {:16x}   RDI: {:16x}   R8:  {:16x} |",
            self.registers[6], self.registers[7], self.registers[8]);
        writeln!(result, "| R9:  {:16x}   R10: {:16x}   R11: {:16x} |",
            self.registers[9], self.registers[10], self.registers[11]);
        writeln!(result, "| R12: {:16x}   R13: {:16x}   R14: {:16x} |",
            self.registers[9], self.registers[10], self.registers[11]);
    }

    fn dump_bank<W: Write>(&self, result: &mut W, bank: &RegisterBank) {
        let mut line_loc = 0;
        let max_loc = 71;
        let bank_stalled = self.values.get(&bank.stall_signal).unwrap().is_true();
        let bank_bubbled = self.values.get(&bank.bubble_signal).unwrap().is_true();
        let status = if bank_bubbled { 'B' } else if bank_bubbled { 'S' } else { 'N' };
        write!(result, "| register {}({}) {{", bank.label, status);
        line_loc += 18;
        for signal in &bank.signals {
            let name = signal.0.split_at(2).1;
            let width = signal.2;
            let hex_width = ((width.bits_or_128() + 3) / 4) as usize;
            if line_loc + 2 + hex_width + name.len() >= max_loc {
                while line_loc < max_loc {
                    write!(result, " ");
                    line_loc += 1;
                }
                write!(result, " |\n| ");
                line_loc = 2;
            }
            let value = self.values.get(&signal.1).unwrap().bits;
            write!(result, " {}={:hex_width$x}", name, value, hex_width=hex_width);
            line_loc += 2 + hex_width + name.len();
        }
        if line_loc + 2 >= max_loc {
            while line_loc < max_loc {
                write!(result, " ");
                line_loc += 1;
            }
            write!(result, " |\n| ");
            line_loc = 2;
        }
        write!(result, " }}");
        line_loc += 2;
        while line_loc < max_loc {
            write!(result, " ");
            line_loc += 1;
        }
        write!(result, " |\n");
    }

    fn dump_custom_registers_y86<W: Write>(&self, result: &mut W) {
        let mut banks_by_letter: HashMap<char, &RegisterBank> = HashMap::new();
        if self.program.register_banks.len() == 0 {
            return;
        }
        for bank in &self.program.register_banks {
            let letter = bank.stall_signal.chars().last().unwrap();
            banks_by_letter.insert(letter, bank);
        }
        let order = ['P', 'F', 'D', 'E', 'M', 'W'];
        for letter in order.iter() {
            if let Some(bank) = banks_by_letter.get(letter) {
                self.dump_bank(result, bank);
            }
        }
        for letter in order.iter() {
            banks_by_letter.remove(letter);
        }
        let mut letters: Vec<char> = banks_by_letter.keys().map(|&x| x).collect();
        letters.sort();
        for letter in letters {
            let bank = banks_by_letter.get(&letter).unwrap();
            self.dump_bank(result, bank);
        }
    }

    fn dump_memory_y86<W: Write>(&self, result: &mut W) {
        writeln!(result,   "| used memory:   _0 _1 _2 _3  _4 _5 _6 _7   _8 _9 _a _b  _c _d _e _f    |");
        let mut cur_addr: u64 = 0;
        for (&k, &v) in &self.memory.data {
            while cur_addr <= k {
                if cur_addr % 16 == 0 {
                    write!(result, "\n|    {:07x}:  ", cur_addr);
                }
                if cur_addr == k {
                    write!(result, " {:02x}", v);
                } else {
                    write!(result, "   ");
                }
                match cur_addr % 16 {
                    3 | 11 => {write!(result, " ");},
                    7 => {write!(result, "  ");},
                    _ => {},
                }
                if cur_addr % 16 == 15 {
                    write!(result, "    |\n");
                    /* potentially skip ahead */
                    if k != cur_addr {
                        cur_addr = (k >> 4) << 4;
                    } else {
                        cur_addr += 1;
                    }
                }  else {
                    cur_addr += 1;
                }
            }
        }
        while cur_addr % 16 != 0 {
            match cur_addr % 16 {
                15 => write!(result, "      |\n"),
                3 | 11 => write!(result, "     "),
                7 => write!(result, "     "),
                _ => write!(result, "   "),
            };
        }
    }

    pub fn dump_y86(&self) -> String {
        let mut result: Vec<u8> = Vec::new();
        // FIXME: copy y86 format
        if self.halted() {
            writeln!(result,
                "+----------------------- halted in state: ------------------------------+"
            );
        } else if self.done() {
            writeln!(result,
                "+------------------- error caused in state: ----------------------------+"
            );
        } else if self.timed_out() {
            writeln!(result,
                "+------------ timed out after {:5} cycles in state: -------------------+",
                self.cycle
            );
        } else {
            writeln!(result,
                "+------------------- between cycles {:4} and {:4} ----------------------+",
                self.cycle, self.cycle + 1
            );
        }
        self.dump_program_registers_y86(&mut result);
        self.dump_custom_registers_y86(&mut result);
        if self.halted() {
            writeln!(result,
                "+--------------------- (end of halted state) ---------------------------+"
            );
        } else if self.done() {
            writeln!(result,
                "+-------------------- (end of error state) -----------------------------+"
            );
        } else {
            writeln!(result,
                "+-----------------------------------------------------------------------+"
            );
        }
        String::from_utf8_lossy(result.as_slice()).into_owned()
    }

    pub fn dump(&self) -> String {
        format!("{:?}", self.values)
    }
}
