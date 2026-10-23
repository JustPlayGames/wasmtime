use cranelift_codegen::ir::*;
use cranelift_codegen::isa::CallConv;
use cranelift_codegen::settings::{self, Configurable};
use cranelift_codegen::{Context, ir::types::I16};
use cranelift_entity::EntityRef;
use cranelift_frontend::*;
use cranelift_module::*;
use cranelift_object::*;

#[test]
fn error_on_incompatible_sig_in_declare_function() {
    let flag_builder = settings::builder();
    let isa_builder = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu").unwrap();
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "foo", default_libcall_names()).unwrap());
    let mut sig = Signature {
        params: vec![AbiParam::new(types::I64)],
        returns: vec![],
        call_conv: CallConv::SystemV,
    };
    module
        .declare_function("abc", Linkage::Local, &sig)
        .unwrap();
    sig.params[0] = AbiParam::new(types::I32);
    module
        .declare_function("abc", Linkage::Local, &sig)
        .err()
        .unwrap(); // Make sure this is an error
}

fn define_simple_function(module: &mut ObjectModule) -> FuncId {
    let sig = Signature {
        params: vec![],
        returns: vec![],
        call_conv: CallConv::SystemV,
    };

    let func_id = module
        .declare_function("abc", Linkage::Local, &sig)
        .unwrap();

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx: FunctionBuilder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let block = bcx.create_block();
        bcx.switch_to_block(block);
        bcx.ins().return_(&[]);
    }

    module.define_function(func_id, &mut ctx).unwrap();

    func_id
}

#[test]
#[should_panic(expected = "Result::unwrap()` on an `Err` value: DuplicateDefinition(\"abc\")")]
fn panic_on_define_after_finalize() {
    let flag_builder = settings::builder();
    let isa_builder = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu").unwrap();
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "foo", default_libcall_names()).unwrap());

    define_simple_function(&mut module);
    define_simple_function(&mut module);
}

#[test]
#[cfg_attr(not(debug_assertions), ignore = "checks a debug assertion")]
#[should_panic(expected = "function \"abc\" with linkage Local must be defined but is not")]
fn panic_on_declare_without_define() {
    let flag_builder = settings::builder();
    let isa_builder = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu").unwrap();
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "foo", default_libcall_names()).unwrap());

    module
        .declare_function("abc", Linkage::Local, &Signature::new(CallConv::SystemV))
        .unwrap();

    module.finish();
}

#[test]
fn switch_error() {
    use cranelift_codegen::settings;

    let sig = Signature {
        params: vec![AbiParam::new(types::I32)],
        returns: vec![AbiParam::new(types::I32)],
        call_conv: CallConv::SystemV,
    };

    let mut func = Function::with_name_signature(UserFuncName::default(), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx: FunctionBuilder = FunctionBuilder::new(&mut func, &mut func_ctx);
        let start = bcx.create_block();
        let bb0 = bcx.create_block();
        let bb1 = bcx.create_block();
        let bb2 = bcx.create_block();
        let bb3 = bcx.create_block();
        println!("{start} {bb0} {bb1} {bb2} {bb3}");

        bcx.declare_var(types::I32);
        bcx.declare_var(types::I32);
        let in_val = bcx.append_block_param(start, types::I32);
        bcx.switch_to_block(start);
        bcx.def_var(Variable::new(0), in_val);
        bcx.ins().jump(bb0, &[]);

        bcx.switch_to_block(bb0);
        let discr = bcx.use_var(Variable::new(0));
        let mut switch = cranelift_frontend::Switch::new();
        for &(index, bb) in &[
            (9, bb1),
            (13, bb1),
            (10, bb1),
            (92, bb1),
            (39, bb1),
            (34, bb1),
        ] {
            switch.set_entry(index, bb);
        }
        switch.emit(&mut bcx, discr, bb2);

        bcx.switch_to_block(bb1);
        let v = bcx.use_var(Variable::new(0));
        bcx.def_var(Variable::new(1), v);
        bcx.ins().jump(bb3, &[]);

        bcx.switch_to_block(bb2);
        let v = bcx.use_var(Variable::new(0));
        bcx.def_var(Variable::new(1), v);
        bcx.ins().jump(bb3, &[]);

        bcx.switch_to_block(bb3);
        let r = bcx.use_var(Variable::new(1));
        bcx.ins().return_(&[r]);

        bcx.seal_all_blocks();
        bcx.finalize(cranelift_codegen::isa::TargetFrontendConfig {
            default_call_conv: CallConv::SystemV,
            pointer_width: target_lexicon::PointerWidth::U64,
            page_size_align_log2: 12,
        });
    }

    let flags = settings::Flags::new(settings::builder());
    match cranelift_codegen::verify_function(&func, &flags) {
        Ok(_) => {}
        Err(err) => {
            let pretty_error =
                cranelift_codegen::print_errors::pretty_verifier_error(&func, None, err);
            panic!("pretty_error:\n{pretty_error}");
        }
    }
}

#[test]
fn libcall_function() {
    let flag_builder = settings::builder();
    let isa_builder = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu").unwrap();
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "foo", default_libcall_names()).unwrap());

    let sig = Signature {
        params: vec![],
        returns: vec![],
        call_conv: CallConv::SystemV,
    };

    let func_id = module
        .declare_function("function", Linkage::Local, &sig)
        .unwrap();

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx: FunctionBuilder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let block = bcx.create_block();
        bcx.switch_to_block(block);

        let int = module.target_config().pointer_type();
        let zero = bcx.ins().iconst(I16, 0);
        let size = bcx.ins().iconst(int, 10);

        let mut signature = module.make_signature();
        signature.params.push(AbiParam::new(int));
        signature.returns.push(AbiParam::new(int));
        let callee = module
            .declare_function("malloc", Linkage::Import, &signature)
            .expect("declare malloc function");
        let local_callee = module.declare_func_in_func(callee, &mut bcx.func);
        let argument_exprs = vec![size];
        let call = bcx.ins().call(local_callee, &argument_exprs);
        let buffer = bcx.inst_results(call)[0];

        bcx.call_memset(module.target_config(), buffer, zero, size);

        bcx.ins().return_(&[]);
    }

    module.define_function(func_id, &mut ctx).unwrap();

    module.finish();
}

#[test]
#[should_panic(expected = "has a null byte, which is disallowed")]
fn reject_nul_byte_symbol_for_func() {
    let flag_builder = settings::builder();
    let isa_builder = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu").unwrap();
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "foo", default_libcall_names()).unwrap());

    let sig = Signature {
        params: vec![],
        returns: vec![],
        call_conv: CallConv::SystemV,
    };

    let _ = module
        .declare_function("function\u{0}with\u{0}nul\u{0}bytes", Linkage::Local, &sig)
        .unwrap();
}

#[test]
#[should_panic(expected = "has a null byte, which is disallowed")]
fn reject_nul_byte_symbol_for_data() {
    let flag_builder = settings::builder();
    let isa_builder = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu").unwrap();
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "foo", default_libcall_names()).unwrap());

    let _ = module
        .declare_data(
            "data\u{0}with\u{0}nul\u{0}bytes",
            Linkage::Local,
            /* writable = */ true,
            /* tls = */ false,
        )
        .unwrap();
}

#[test]
fn aarch64_colocated_data_symbol_reloc() {
    let flag_builder = settings::builder();
    let isa_builder = cranelift_codegen::isa::lookup_by_name("aarch64-unknown-linux-gnu").unwrap();
    let isa = isa_builder
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap());

    let data_id = module
        .declare_data("my_data", Linkage::Local, true, false)
        .unwrap();

    let mut data_desc = DataDescription::new();
    data_desc.define_zeroinit(64);
    module.define_data(data_id, &data_desc).unwrap();

    let sig = Signature {
        params: vec![],
        returns: vec![AbiParam::new(types::I64)],
        call_conv: CallConv::SystemV,
    };

    let func_id = module
        .declare_function("load_data_addr", Linkage::Local, &sig)
        .unwrap();

    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx: FunctionBuilder = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let block = bcx.create_block();
        bcx.switch_to_block(block);

        let gv = module.declare_data_in_func(data_id, &mut bcx.func);
        let ptr = module.target_config().pointer_type();
        let addr = bcx.ins().symbol_value(ptr, gv);
        bcx.ins().return_(&[addr]);

        bcx.seal_all_blocks();
        bcx.finalize(module.target_config());
    }

    module.define_function(func_id, &mut ctx).unwrap();

    let product = module.finish();
    product.emit().expect("emit object file");
}

#[test]
fn x86_64_colocated_symbol_address_uses_non_branch_relocation() {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};

    for (triple, expected_encoding) in [
        (
            "x86_64-unknown-linux-gnu",
            object::RelocationEncoding::Generic,
        ),
        (
            "x86_64-apple-darwin",
            object::RelocationEncoding::X86RipRelative,
        ),
        (
            "x86_64-pc-windows-msvc",
            object::RelocationEncoding::Generic,
        ),
    ] {
        let flag_builder = settings::builder();
        let isa = cranelift_codegen::isa::lookup_by_name(triple)
            .unwrap()
            .finish(settings::Flags::new(flag_builder))
            .unwrap();
        let mut module =
            ObjectModule::new(ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap());

        let data_id = module
            .declare_data("my_data", Linkage::Local, true, false)
            .unwrap();
        let mut data_desc = DataDescription::new();
        data_desc.define_zeroinit(8);
        module.define_data(data_id, &data_desc).unwrap();

        let sig = Signature {
            params: vec![],
            returns: vec![AbiParam::new(types::I64)],
            call_conv: module.target_config().default_call_conv,
        };
        let func_id = module
            .declare_function("data_address", Linkage::Local, &sig)
            .unwrap();
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
        let mut func_ctx = FunctionBuilderContext::new();
        {
            let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
            let block = bcx.create_block();
            bcx.switch_to_block(block);
            let data = module.declare_data_in_func(data_id, &mut bcx.func);
            let address = bcx
                .ins()
                .symbol_value(module.target_config().pointer_type(), data);
            bcx.ins().return_(&[address]);
            bcx.seal_all_blocks();
            bcx.finalize(module.target_config());
        }
        module.define_function(func_id, &mut ctx).unwrap();

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        let data_symbol = file
            .symbols()
            .find(|symbol| {
                symbol
                    .name()
                    .is_ok_and(|name| name.trim_start_matches('_') == "my_data")
            })
            .unwrap_or_else(|| panic!("{triple}: data symbol present in object"))
            .index();
        let function = file
            .symbols()
            .find(|symbol| {
                symbol
                    .name()
                    .is_ok_and(|name| name.trim_start_matches('_') == "data_address")
            })
            .unwrap_or_else(|| panic!("{triple}: function symbol present in object"));
        let section = file
            .section_by_index(function.section_index().unwrap())
            .expect("function section present");
        let (_, relocation) = section
            .relocations()
            .find(|(_, relocation)| {
                relocation.target() == object::read::RelocationTarget::Symbol(data_symbol)
            })
            .expect("address materialization relocation present");

        assert_eq!(
            relocation.kind(),
            object::RelocationKind::Relative,
            "{triple}"
        );
        assert_eq!(
            relocation.encoding(),
            expected_encoding,
            "{triple}: an address materialization must not use a branch relocation"
        );
        assert_eq!(relocation.size(), 32, "{triple}");
    }
}

// ---------- `.eh_frame` emission tests ----------

#[cfg(feature = "unwind")]
mod eh_frame {
    use super::*;
    use cranelift_codegen::settings::Configurable as _;
    use gimli::UnwindSection as _;
    use object::Object as _;
    use object::ObjectSection as _;

    /// Build an `ObjectModule` for `triple` with the unwind-info builder flag
    /// set to `unwind_info`. Frame pointers are forced on so cranelift emits
    /// a non-trivial prologue (and therefore unwind info) for every function.
    fn module_for(triple: &str, unwind_info: bool) -> ObjectModule {
        let mut flag_builder = settings::builder();
        flag_builder.set("preserve_frame_pointers", "true").unwrap();
        let isa = cranelift_codegen::isa::lookup_by_name(triple)
            .unwrap()
            .finish(settings::Flags::new(flag_builder))
            .unwrap();
        let mut builder = ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap();
        builder.unwind_info(unwind_info);
        ObjectModule::new(builder)
    }

    /// Define a leaf function that just returns. With `preserve_frame_pointers`
    /// on, this is enough to make cranelift emit a System V FDE for it.
    fn define_leaf(module: &mut ObjectModule, name: &str) -> FuncId {
        let sig = Signature {
            params: vec![],
            returns: vec![],
            call_conv: CallConv::SystemV,
        };
        let func_id = module.declare_function(name, Linkage::Local, &sig).unwrap();
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
        let mut func_ctx = FunctionBuilderContext::new();
        {
            let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
            let block = bcx.create_block();
            bcx.switch_to_block(block);
            bcx.ins().return_(&[]);
            bcx.seal_all_blocks();
            bcx.finalize(module.target_config());
        }
        module.define_function(func_id, &mut ctx).unwrap();
        func_id
    }

    /// Iterate the entries in `.eh_frame` bytes, returning `(cie_count, fde_count)`.
    fn count_entries(data: &[u8]) -> (usize, usize) {
        let mut eh_frame = gimli::EhFrame::new(data, gimli::LittleEndian);
        eh_frame.set_address_size(8);
        let bases = gimli::BaseAddresses::default();
        let mut entries = eh_frame.entries(&bases);
        let (mut cies, mut fdes) = (0, 0);
        while let Some(entry) = entries.next().expect("walk eh_frame entries") {
            match entry {
                gimli::CieOrFde::Cie(_) => cies += 1,
                gimli::CieOrFde::Fde(_) => fdes += 1,
            }
        }
        (cies, fdes)
    }

    #[test]
    fn emits_eh_frame_for_x86_64_systemv_target() {
        let mut module = module_for("x86_64-unknown-linux-gnu", true);
        define_leaf(&mut module, "leaf");

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        let section = file
            .section_by_name(".eh_frame")
            .expect(".eh_frame section is present");
        let data = section.data().expect("read .eh_frame data");
        assert!(!data.is_empty(), ".eh_frame must not be empty");

        let (cies, fdes) = count_entries(data);
        assert_eq!(cies, 1, "expected exactly one CIE");
        assert_eq!(fdes, 1, "expected exactly one FDE");

        let relocations: Vec<_> = section.relocations().collect();
        assert!(
            !relocations.is_empty(),
            ".eh_frame must carry symbol relocations"
        );
    }

    #[test]
    fn emits_eh_frame_for_aarch64_target() {
        let mut module = module_for("aarch64-unknown-linux-gnu", true);
        define_leaf(&mut module, "leaf");

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        let data = file
            .section_by_name(".eh_frame")
            .expect(".eh_frame section is present")
            .data()
            .expect("read .eh_frame data");

        let (cies, fdes) = count_entries(data);
        assert_eq!(cies, 1, "expected exactly one CIE");
        assert_eq!(fdes, 1, "expected exactly one FDE");
    }

    #[test]
    fn unwind_info_disabled_by_default_emits_no_eh_frame() {
        let flag_builder = settings::builder();
        let isa = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu")
            .unwrap()
            .finish(settings::Flags::new(flag_builder))
            .unwrap();
        let mut module =
            ObjectModule::new(ObjectBuilder::new(isa, "no_eh", default_libcall_names()).unwrap());
        define_simple_function(&mut module);

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        assert!(
            file.section_by_name(".eh_frame").is_none(),
            "no .eh_frame section should be emitted when unwind_info is off"
        );
    }

    #[test]
    fn cie_is_shared_across_multiple_functions() {
        let mut module = module_for("x86_64-unknown-linux-gnu", true);
        define_leaf(&mut module, "leaf_a");
        define_leaf(&mut module, "leaf_b");
        define_leaf(&mut module, "leaf_c");

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        let data = file
            .section_by_name(".eh_frame")
            .expect(".eh_frame section is present")
            .data()
            .expect("read .eh_frame data");

        let (cies, fdes) = count_entries(data);
        assert_eq!(cies, 1, "all functions must share a single CIE, got {cies}");
        assert_eq!(fdes, 3, "expected one FDE per function, got {fdes}");
    }

    #[test]
    fn fde_relocations_target_function_symbols() {
        use object::ObjectSymbol as _;

        let mut module = module_for("x86_64-unknown-linux-gnu", true);
        define_leaf(&mut module, "leaf");

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        let section = file
            .section_by_name(".eh_frame")
            .expect(".eh_frame section is present");

        let leaf_symbol_index = file
            .symbols()
            .find(|s| s.name() == Ok("leaf"))
            .expect("function symbol present in object")
            .index();

        let (_, reloc) = section
            .relocations()
            .next()
            .expect(".eh_frame must carry at least one relocation");
        let object::read::RelocationTarget::Symbol(target_index) = reloc.target() else {
            panic!(
                "expected symbol-targeted relocation, got {:?}",
                reloc.target()
            );
        };
        assert_eq!(
            target_index, leaf_symbol_index,
            "FDE relocation must target the function's text symbol"
        );
    }

    #[test]
    fn per_function_section_combines_with_unwind_info() {
        // With every function in its own subsection, the FDE relocations must
        // still resolve because they target function symbols rather than the
        // shared `.text` section. Just walking the entries should succeed and
        // produce one FDE per function.
        let mut flag_builder = settings::builder();
        flag_builder.set("preserve_frame_pointers", "true").unwrap();
        let isa = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu")
            .unwrap()
            .finish(settings::Flags::new(flag_builder))
            .unwrap();
        let mut builder = ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap();
        builder.unwind_info(true);
        builder.per_function_section(true);
        let mut module = ObjectModule::new(builder);
        define_leaf(&mut module, "leaf_a");
        define_leaf(&mut module, "leaf_b");

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        let data = file
            .section_by_name(".eh_frame")
            .expect(".eh_frame section is present")
            .data()
            .expect("read .eh_frame data");
        let (cies, fdes) = count_entries(data);
        assert_eq!(cies, 1);
        assert_eq!(fdes, 2);
    }

    #[test]
    fn no_eh_frame_emitted_for_windows_target() {
        // On Windows targets cranelift produces `UnwindInfo::WindowsX64`
        // rather than System V info, and the unwind builder ignores those
        // variants. With no FDE ever added, no `.eh_frame` section should
        // appear in the resulting object even though `unwind_info(true)` was
        // set on the builder.
        let mut module = module_for("x86_64-pc-windows-msvc", true);
        let sig = Signature {
            params: vec![],
            returns: vec![],
            call_conv: CallConv::WindowsFastcall,
        };
        let func_id = module
            .declare_function("leaf", Linkage::Local, &sig)
            .unwrap();
        let mut ctx = Context::new();
        ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
        let mut func_ctx = FunctionBuilderContext::new();
        {
            let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
            let block = bcx.create_block();
            bcx.switch_to_block(block);
            bcx.ins().return_(&[]);
            bcx.seal_all_blocks();
            bcx.finalize(module.target_config());
        }
        module.define_function(func_id, &mut ctx).unwrap();

        let bytes = module.finish().emit().expect("emit object file");
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        assert!(
            file.section_by_name(".eh_frame").is_none(),
            "no .eh_frame should be emitted for Windows targets"
        );
    }
}

#[test]
fn x86_64_elf_pic_symbol_address_relocations() {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};

    // Under `is_pic`, symbols are addressed through GOT loads the linker may relax
    // (`R_X86_64_REX_GOTPCRELX`); a local symbol's load is relaxed to a `lea` already.
    let mut flag_builder = settings::builder();
    flag_builder.set("is_pic", "true").unwrap();
    let isa = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu")
        .unwrap()
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap());

    let local_id = module
        .declare_data("local_data", Linkage::Local, true, false)
        .unwrap();
    let mut data_desc = DataDescription::new();
    data_desc.define_zeroinit(8);
    module.define_data(local_id, &data_desc).unwrap();
    let import_id = module
        .declare_data("imported_data", Linkage::Import, true, false)
        .unwrap();

    let sig = Signature {
        params: vec![],
        returns: vec![AbiParam::new(types::I64)],
        call_conv: module.target_config().default_call_conv,
    };
    let func_id = module
        .declare_function("addresses", Linkage::Local, &sig)
        .unwrap();
    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let block = bcx.create_block();
        bcx.switch_to_block(block);
        let pointer_type = module.target_config().pointer_type();
        let local = module.declare_data_in_func(local_id, &mut bcx.func);
        let local = bcx.ins().symbol_value(pointer_type, local);
        let import = module.declare_data_in_func(import_id, &mut bcx.func);
        let import = bcx.ins().symbol_value(pointer_type, import);
        let sum = bcx.ins().iadd(local, import);
        bcx.ins().return_(&[sum]);
        bcx.seal_all_blocks();
        bcx.finalize(module.target_config());
    }
    module.define_function(func_id, &mut ctx).unwrap();

    let bytes = module.finish().emit().expect("emit object file");
    let file = object::File::parse(&*bytes).expect("parse emitted object");
    let symbol = |name: &str| {
        file.symbols()
            .find(|symbol| symbol.name() == Ok(name))
            .unwrap_or_else(|| panic!("symbol {name} present in object"))
    };
    let function = symbol("addresses");
    let section = file
        .section_by_index(function.section_index().unwrap())
        .expect("function section present");
    let r_type = |target: object::SymbolIndex| {
        let (_, relocation) = section
            .relocations()
            .find(|(_, relocation)| {
                relocation.target() == object::read::RelocationTarget::Symbol(target)
            })
            .expect("relocation present");
        match relocation.flags() {
            object::RelocationFlags::Elf { r_type } => r_type,
            flags => panic!("unexpected relocation flags {flags:?}"),
        }
    };
    assert_eq!(
        r_type(symbol("local_data").index()),
        object::elf::R_X86_64_PC32
    );
    assert_eq!(
        r_type(symbol("imported_data").index()),
        object::elf::R_X86_64_REX_GOTPCRELX
    );
}

/// A module with one anonymous constant (8 bytes, after 16 bytes of other anonymous data), a
/// function returning its address, and a pointer to it in anonymous data.
fn anonymous_data_object(triple: &str, is_pic: bool) -> Vec<u8> {
    let mut flag_builder = settings::builder();
    flag_builder
        .set("is_pic", if is_pic { "true" } else { "false" })
        .unwrap();
    let isa = cranelift_codegen::isa::lookup_by_name(triple)
        .unwrap()
        .finish(settings::Flags::new(flag_builder))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap());

    let first = module.declare_anonymous_data(false, false).unwrap();
    let mut data_desc = DataDescription::new();
    data_desc.define(vec![1; 16].into_boxed_slice());
    module.define_data(first, &data_desc).unwrap();

    let constant = module.declare_anonymous_data(false, false).unwrap();
    let mut data_desc = DataDescription::new();
    data_desc.define(vec![2; 8].into_boxed_slice());
    data_desc.set_align(8);
    module.define_data(constant, &data_desc).unwrap();

    let pointer = module.declare_anonymous_data(false, false).unwrap();
    let mut data_desc = DataDescription::new();
    data_desc.define(vec![0; 8].into_boxed_slice());
    let gv = module.declare_data_in_data(constant, &mut data_desc);
    data_desc.write_data_addr(0, gv, 4);
    module.define_data(pointer, &data_desc).unwrap();

    let sig = Signature {
        params: vec![],
        returns: vec![AbiParam::new(types::I64)],
        call_conv: module.target_config().default_call_conv,
    };
    let func_id = module
        .declare_function("address", Linkage::Export, &sig)
        .unwrap();
    let mut ctx = Context::new();
    ctx.func = Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig);
    let mut func_ctx = FunctionBuilderContext::new();
    {
        let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
        let block = bcx.create_block();
        bcx.switch_to_block(block);
        let gv = module.declare_data_in_func(constant, &mut bcx.func);
        let address = bcx
            .ins()
            .symbol_value(module.target_config().pointer_type(), gv);
        bcx.ins().return_(&[address]);
        bcx.seal_all_blocks();
        bcx.finalize(module.target_config());
    }
    module.define_function(func_id, &mut ctx).unwrap();

    module.finish().emit().expect("emit object file")
}

#[test]
fn anonymous_data_is_addressed_through_its_section() {
    use object::{Object as _, ObjectSection as _, ObjectSymbol as _};

    for (triple, is_pic) in [
        ("x86_64-unknown-linux-gnu", true),
        ("x86_64-unknown-linux-gnu", false),
        ("x86_64-pc-windows-msvc", false),
        ("aarch64-unknown-linux-gnu", false),
    ] {
        let bytes = anonymous_data_object(triple, is_pic);
        let file = object::File::parse(&*bytes).expect("parse emitted object");
        assert!(
            file.symbols()
                .all(|symbol| !symbol.name().unwrap().starts_with(".Ldata")),
            "{triple}: anonymous data has no symbols"
        );
        let rodata = file
            .sections()
            .find(|section| section.data().unwrap().starts_with(&[1; 16]))
            .expect("read-only data section");

        // The relocations of the function and of the pointer both target the section of
        // the constant, at its offset.
        let mut targets = 0;
        for section in file.sections() {
            for (offset, relocation) in section.relocations() {
                let object::RelocationTarget::Symbol(symbol) = relocation.target() else {
                    continue;
                };
                let symbol = file.symbol_by_index(symbol).unwrap();
                assert_eq!(
                    symbol.kind(),
                    object::SymbolKind::Section,
                    "{triple}: relocation at {offset:#x} of {} targets a section",
                    section.name().unwrap()
                );
                assert_eq!(symbol.section_index(), Some(rodata.index()));
                let addend = if relocation.has_implicit_addend() {
                    let data = section.data().unwrap();
                    let at = offset as usize;
                    match relocation.size() {
                        32 => i64::from(i32::from_le_bytes(data[at..at + 4].try_into().unwrap())),
                        64 => i64::from_le_bytes(data[at..at + 8].try_into().unwrap()),
                        size => panic!("{triple}: relocation size {size}"),
                    }
                } else {
                    relocation.addend()
                };
                let expected = match (section.kind(), relocation.kind()) {
                    // The pointer's addend of 4.
                    (_, object::RelocationKind::Absolute) => 16 + 4,
                    // `lea` and `adrp`/`add`: the displacement is from the end of the
                    // instruction on x86-64 (COFF's REL32 holds that in its type).
                    _ if triple.starts_with("x86_64") && triple.contains("linux") => 16 - 4,
                    _ => 16,
                };
                assert_eq!(addend, expected, "{triple}: addend at {offset:#x}");
                targets += 1;
            }
        }
        assert!(targets >= 2, "{triple}: {targets} relocations");

        // On x86-64 ELF under `is_pic` the GOT load became a `lea`.
        if triple == "x86_64-unknown-linux-gnu" && is_pic {
            let address = file
                .symbols()
                .find(|symbol| symbol.name() == Ok("address"))
                .unwrap();
            let text = file
                .section_by_index(address.section_index().unwrap())
                .unwrap();
            let (offset, _) = text.relocations().next().unwrap();
            assert_eq!(text.data().unwrap()[offset as usize - 2], 0x8d, "lea");
        }
    }
}

#[test]
fn anonymous_data_symbol_on_request() {
    // A relocation that needs a symbol (a GOT load of a TLS-free anonymous object can't occur
    // here, so ask through the product) gets a local one at the data's offset.
    let isa = cranelift_codegen::isa::lookup_by_name("x86_64-unknown-linux-gnu")
        .unwrap()
        .finish(settings::Flags::new(settings::builder()))
        .unwrap();
    let mut module =
        ObjectModule::new(ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap());
    let first = module.declare_anonymous_data(true, false).unwrap();
    let mut data_desc = DataDescription::new();
    data_desc.define_zeroinit(24);
    module.define_data(first, &data_desc).unwrap();
    let second = module.declare_anonymous_data(true, false).unwrap();
    let mut data_desc = DataDescription::new();
    data_desc.define_zeroinit(8);
    data_desc.set_align(8);
    module.define_data(second, &data_desc).unwrap();

    let mut product = module.finish();
    let (section_symbol, offset) = product.data_symbol_and_offset(second);
    assert_eq!(offset, 24);
    assert_eq!(
        product.object.symbol(section_symbol).kind,
        object::SymbolKind::Section
    );
    let symbol = product.data_symbol(second);
    assert_eq!(product.object.symbol(symbol).value, 24);
    assert_eq!(product.object.symbol(symbol).size, 8);
    assert_eq!(product.data_symbol(second), symbol);
    product.emit().expect("emit object file");
}

#[test]
fn coff_function_and_data_sections_are_comdats() {
    use object::{Object as _, ObjectComdat as _, ObjectSection as _, ObjectSymbol as _};

    let isa = cranelift_codegen::isa::lookup_by_name("x86_64-pc-windows-msvc")
        .unwrap()
        .finish(settings::Flags::new(settings::builder()))
        .unwrap();
    let mut builder = ObjectBuilder::new(isa, "test", default_libcall_names()).unwrap();
    builder.per_function_section(true);
    builder.per_data_object_section(true);
    let mut module = ObjectModule::new(builder);

    let constant = module.declare_anonymous_data(false, false).unwrap();
    let mut data_desc = DataDescription::new();
    data_desc.define(vec![7; 8].into_boxed_slice());
    module.define_data(constant, &data_desc).unwrap();

    let sig = Signature {
        params: vec![],
        returns: vec![AbiParam::new(types::I64)],
        call_conv: module.target_config().default_call_conv,
    };
    let mut functions = vec![];
    for name in ["first", "second"] {
        let func_id = module
            .declare_function(name, Linkage::Export, &sig)
            .unwrap();
        let mut ctx = Context::new();
        ctx.func =
            Function::with_name_signature(UserFuncName::user(0, func_id.as_u32()), sig.clone());
        let mut func_ctx = FunctionBuilderContext::new();
        {
            let mut bcx = FunctionBuilder::new(&mut ctx.func, &mut func_ctx);
            let block = bcx.create_block();
            bcx.switch_to_block(block);
            let gv = module.declare_data_in_func(constant, &mut bcx.func);
            let address = bcx
                .ins()
                .symbol_value(module.target_config().pointer_type(), gv);
            bcx.ins().return_(&[address]);
            bcx.seal_all_blocks();
            bcx.finalize(module.target_config());
        }
        module.define_function(func_id, &mut ctx).unwrap();
        functions.push(func_id);
    }

    let mut product = module.finish();
    let first_section = product
        .object
        .symbol(product.function_symbol(functions[0]))
        .section
        .id()
        .unwrap();
    assert!(product.section_comdat(first_section).is_some());
    let bytes = product.emit().expect("emit object file");

    let file = object::File::parse(&*bytes).expect("parse emitted object");
    // One COMDAT per function and one for the constant, each with a section of its own.
    let comdats: Vec<_> = file.comdats().collect();
    assert_eq!(comdats.len(), 3);
    for comdat in &comdats {
        assert_eq!(comdat.kind(), object::ComdatKind::NoDuplicates);
        assert_eq!(comdat.sections().count(), 1);
    }
    for name in ["first", "second"] {
        let symbol = file.symbols().find(|s| s.name() == Ok(name)).unwrap();
        assert!(symbol.is_global());
        let section = file
            .section_by_index(symbol.section_index().unwrap())
            .unwrap();
        assert_eq!(section.name(), Ok(".text"));
        assert!(
            comdats
                .iter()
                .any(|comdat| comdat.sections().any(|s| s == section.index()))
        );
    }
}
