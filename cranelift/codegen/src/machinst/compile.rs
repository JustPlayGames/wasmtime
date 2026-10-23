//! Compilation backend pipeline: optimized IR to VCode / binemit.

use crate::dominator_tree::DominatorTree;
use crate::ir::Function;
use crate::isa::TargetIsa;
use crate::machinst::*;
use crate::settings::RegallocAlgorithm;
use crate::timing;
use crate::trace;

use regalloc2::{Algorithm, RegallocOptions};

/// Compile the given function down to VCode with allocated registers, ready
/// for binary emission.
pub fn compile<B: LowerBackend + TargetIsa>(
    f: &Function,
    domtree: &DominatorTree,
    regalloc_ctx: &mut regalloc2::Ctx,
    b: &B,
    abi: Callee<<<B as LowerBackend>::MInst as MachInst>::ABIMachineSpec>,
    emit_info: <B::MInst as MachInstEmit>::Info,
    sigs: SigSet,
    ctrl_plane: &mut ControlPlane,
) -> CodegenResult<VCode<B::MInst>> {
    // Compute lowered block order.
    let block_order = BlockLoweringOrder::new(f, domtree, ctrl_plane);

    // Build the lowering context.
    let lower =
        crate::machinst::Lower::new(f, abi, emit_info, block_order, sigs, b.flags().clone())?;

    // Lower the IR.
    let vcode = {
        log::debug!(
            "Number of CLIF instructions to lower: {}",
            f.dfg.num_insts()
        );
        log::debug!("Number of CLIF blocks to lower: {}", f.dfg.num_blocks());

        let _tt = timing::vcode_lower();
        lower.lower(b, ctrl_plane)?
    };

    log::debug!(
        "Number of lowered vcode instructions: {}",
        vcode.num_insts()
    );
    log::debug!("Number of lowered vcode blocks: {}", vcode.num_blocks());
    trace!("vcode from lowering: \n{:?}", vcode);

    // Perform register allocation.
    {
        let _tt = timing::regalloc();
        let mut options = RegallocOptions::default();
        options.verbose_log = b.flags().regalloc_verbose_logs();

        if cfg!(debug_assertions) {
            options.validate_ssa = true;
        }

        options.algorithm = match f
            .params
            .regalloc_algorithm()
            .unwrap_or_else(|| b.flags().regalloc_algorithm())
        {
            RegallocAlgorithm::Backtracking => Algorithm::Ion,
            RegallocAlgorithm::SinglePass => Algorithm::Fastalloc,
        };

        regalloc2::run_with_ctx(&vcode, vcode.abi.machine_env(), &options, regalloc_ctx)
            .map_err(|err| {
                log::error!(
                    "Register allocation error for vcode\n\
                     {vcode:?}\n\
                     Error: {err:?}\n\
                     CLIF for error:\n\
                     {f:?}",
                );
                err
            })
            .expect("register allocation");
    }

    // Run the regalloc checker, if requested.
    if b.flags().regalloc_checker() {
        let _tt = timing::regalloc_checker();
        let mut checker = regalloc2::checker::Checker::new(&vcode, &vcode.abi.machine_env());
        checker.prepare(&regalloc_ctx.output);
        checker
            .run()
            .map_err(|err| {
                log::error!("Register allocation checker errors:\n{err:?}\nfor vcode:\n{vcode:?}");
                err
            })
            .expect("register allocation checker");
    }

    Ok(vcode)
}

#[cfg(all(test, feature = "x86"))]
mod tests {
    use crate::Context;
    use crate::cursor::{Cursor, FuncCursor};
    use crate::ir::{
        AbiParam, ExtFuncData, ExternalName, Function, InstBuilder, Signature, UserExternalName,
        UserFuncName, types,
    };
    use crate::isa::{CallConv, lookup};
    use crate::settings::{Configurable, Flags, RegallocAlgorithm, builder};
    use alloc::vec::Vec;
    use target_lexicon::triple;

    /// `f(a, b) = { let v = a + b; g(); v + a }`: values live across a call, which the two
    /// allocators handle differently.
    fn function() -> Function {
        let mut sig = Signature::new(CallConv::SystemV);
        sig.params.push(AbiParam::new(types::I64));
        sig.params.push(AbiParam::new(types::I64));
        sig.returns.push(AbiParam::new(types::I64));
        let mut func = Function::with_name_signature(UserFuncName::default(), sig);
        let callee_sig = func.import_signature(Signature::new(CallConv::SystemV));
        let name = func.declare_imported_user_function(UserExternalName::new(0, 1));
        let callee = func.import_function(ExtFuncData {
            name: ExternalName::user(name),
            signature: callee_sig,
            colocated: false,
            patchable: false,
        });
        let block0 = func.dfg.make_block();
        let a = func.dfg.append_block_param(block0, types::I64);
        let b = func.dfg.append_block_param(block0, types::I64);
        let mut pos = FuncCursor::new(&mut func);
        pos.insert_block(block0);
        let v = pos.ins().iadd(a, b);
        pos.ins().call(callee, &[]);
        let r = pos.ins().iadd(v, a);
        pos.ins().return_(&[r]);
        func
    }

    fn compile(isa_algorithm: &str, func_algorithm: Option<RegallocAlgorithm>) -> Vec<u8> {
        let mut flags = builder();
        flags.set("regalloc_algorithm", isa_algorithm).unwrap();
        let isa = lookup(triple!("x86_64"))
            .unwrap()
            .finish(Flags::new(flags))
            .unwrap();
        let mut func = function();
        func.params.set_regalloc_algorithm(func_algorithm);
        let mut ctx = Context::for_function(func);
        ctx.compile(&*isa, &mut Default::default())
            .unwrap()
            .code_buffer()
            .to_vec()
    }

    #[test]
    fn a_function_chooses_its_register_allocator() {
        let backtracking = compile("backtracking", None);
        let single_pass = compile("single_pass", None);
        assert_ne!(
            backtracking, single_pass,
            "the test function should tell the allocators apart"
        );
        assert_eq!(
            compile("backtracking", Some(RegallocAlgorithm::SinglePass)),
            single_pass
        );
        assert_eq!(
            compile("single_pass", Some(RegallocAlgorithm::Backtracking)),
            backtracking
        );
    }
}
