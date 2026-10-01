use super::*;

pub(super) fn spawn_hidden_desktop_process(
    plan: &SpawnCommand<'_>,
    stdout: File,
    stderr: File,
    creation_flags: u32,
    resource_group: Option<&RuntimeResourceGroup>,
) -> Result<(RuntimeChild, Option<WindowsHiddenDesktop>), std::io::Error> {
    let desktop = create_hidden_desktop_for_spawn()?;
    spawn_native_process(
        plan,
        stdout,
        stderr,
        creation_flags,
        Some(desktop),
        true,
        resource_group,
    )
}

pub(super) fn spawn_standard_process(
    plan: &SpawnCommand<'_>,
    stdout: File,
    stderr: File,
    run_in_background: bool,
    uses_script_entrypoint: bool,
    resource_group: Option<&RuntimeResourceGroup>,
) -> Result<(RuntimeChild, Option<WindowsHiddenDesktop>), std::io::Error> {
    let hidden = run_in_background || uses_script_entrypoint;
    let flags = if uses_script_entrypoint {
        CREATE_NO_WINDOW
    } else if run_in_background {
        CREATE_NO_WINDOW | DETACHED_PROCESS
    } else {
        0
    };
    spawn_native_process(plan, stdout, stderr, flags, None, hidden, resource_group)
}

pub(super) fn spawn_elevated_workload(
    plan: &SpawnCommand<'_>,
    stdout: File,
    stderr: File,
    background: bool,
) -> std::io::Result<(RuntimeChild, Option<WindowsHiddenDesktop>)> {
    // An elevated background console remains attachable for the game's graceful
    // stop protocol, while its windows live on the manager's private desktop.
    if background {
        spawn_hidden_desktop_process(plan, stdout, stderr, CREATE_NEW_CONSOLE, None)
    } else {
        spawn_native_process(plan, stdout, stderr, CREATE_NEW_CONSOLE, None, false, None)
    }
}

fn spawn_native_process(
    plan: &SpawnCommand<'_>,
    stdout: File,
    stderr: File,
    creation_flags: u32,
    desktop: Option<WindowsHiddenDesktop>,
    hide_window: bool,
    resource_group: Option<&RuntimeResourceGroup>,
) -> Result<(RuntimeChild, Option<WindowsHiddenDesktop>), std::io::Error> {
    process_environment::validate_windows_command(plan)?;
    let job = windows_process_job::OwnedProcessJob::new_in_resource_group(resource_group)?;
    let mut desktop_name_wide = desktop
        .as_ref()
        .map(|desktop| wide_null(&hidden_desktop_spawn_target(&desktop.name)));
    let mut command_line = wide_null(&build_spawn_command_line(plan.executable, plan.args));
    let mut working_directory_wide = wide_null(&plan.working_directory.to_string_lossy());
    let mut environment = plan.windows_environment();
    let mut stdin_read_raw = std::ptr::null_mut();
    let mut stdin_write_raw = std::ptr::null_mut();
    let mut pipe_security = SecurityAttributes {
        n_length: std::mem::size_of::<SecurityAttributes>() as u32,
        lp_security_descriptor: std::ptr::null_mut(),
        b_inherit_handle: 1,
    };

    if unsafe {
        CreatePipe(
            &mut stdin_read_raw,
            &mut stdin_write_raw,
            &mut pipe_security,
            0,
        )
    } == 0
    {
        let error = std::io::Error::last_os_error();
        close_handle(stdin_read_raw);
        close_handle(stdin_write_raw);
        return Err(error);
    }
    let stdin_read = OwnedWindowsHandle::new(stdin_read_raw);
    let stdin_write = OwnedWindowsHandle::new(stdin_write_raw);
    if unsafe { SetHandleInformation(stdin_write.as_raw(), HANDLE_FLAG_INHERIT, 0) } == 0 {
        return Err(std::io::Error::last_os_error());
    }

    let stdout_child = duplicate_inheritable_handle(stdout.as_raw_handle() as *mut _)?;
    let stderr_child = duplicate_inheritable_handle(stderr.as_raw_handle() as *mut _)?;
    let inherited_handles = hidden_desktop_inherited_handles(
        stdin_read.as_raw(),
        stdout_child.as_raw(),
        stderr_child.as_raw(),
    );
    let mut attribute_list = ProcThreadAttributeList::for_inherited_handles(
        inherited_handles,
        job.launch_job_handles(),
    )?;

    let mut startup = StartupInfoExW {
        startup_info: StartupInfoW {
            cb: std::mem::size_of::<StartupInfoExW>() as u32,
            lp_reserved: std::ptr::null_mut(),
            lp_desktop: desktop_name_wide
                .as_mut()
                .map_or(std::ptr::null_mut(), |name| name.as_mut_ptr()),
            lp_title: std::ptr::null_mut(),
            dw_x: 0,
            dw_y: 0,
            dw_x_size: 0,
            dw_y_size: 0,
            dw_x_count_chars: 0,
            dw_y_count_chars: 0,
            dw_fill_attribute: 0,
            dw_flags: STARTF_USESTDHANDLES | if hide_window { STARTF_USESHOWWINDOW } else { 0 },
            w_show_window: SW_HIDE,
            cb_reserved2: 0,
            lp_reserved2: std::ptr::null_mut(),
            h_std_input: stdin_read.as_raw(),
            h_std_output: stdout_child.as_raw(),
            h_std_error: stderr_child.as_raw(),
        },
        attribute_list: attribute_list.as_mut_raw(),
    };
    let mut process_info = ProcessInformation::default();
    // The sender's temporary Ctrl+C ignore state is process-wide and inherited
    // at creation, even by a child with a new console. Serialize only creation.
    let creation_error = windows_console_control::with_windows_console_ctrl_lock(|| {
        let created = unsafe {
            CreateProcessW(
                std::ptr::null(),
                command_line.as_mut_ptr(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                1,
                creation_flags | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                environment
                    .as_mut()
                    .map_or(std::ptr::null_mut(), |block| block.as_mut_ptr().cast()),
                working_directory_wide.as_mut_ptr(),
                &mut startup.startup_info,
                &mut process_info,
            )
        };
        (created == 0).then(std::io::Error::last_os_error)
    });

    drop(attribute_list);

    if let Some(error) = creation_error {
        return Err(error);
    }
    close_handle(process_info.thread_handle);

    let stdin = unsafe { File::from_raw_handle(stdin_write.into_raw() as *mut _) };
    let child = WindowsSpawnedChild {
        process_handle: process_info.process_handle as usize,
        process_id: process_info.process_id,
        stdin: Some(RuntimeStdin::Windows(WindowsPipeWriter::new(stdin))),
        terminal: None,
        job: Some(job),
        elevated: None,
        output: None,
    };

    Ok((RuntimeChild::Windows(child), desktop))
}

struct ProcThreadAttributeList {
    storage: Vec<usize>,
    _handles: Box<[*mut std::ffi::c_void]>,
    _jobs: Box<[*mut std::ffi::c_void]>,
    initialized: bool,
}

impl ProcThreadAttributeList {
    fn for_inherited_handles<const N: usize>(
        handles: [*mut std::ffi::c_void; N],
        jobs: Box<[*mut std::ffi::c_void]>,
    ) -> Result<Self, std::io::Error> {
        if handles.is_empty() || handles.iter().any(|handle| handle.is_null()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "inherited handle list must contain only valid handles",
            ));
        }

        let mut required_size = 0usize;
        let first_call_succeeded = unsafe {
            InitializeProcThreadAttributeList(std::ptr::null_mut(), 2, 0, &mut required_size)
        } != 0;
        let sizing_error = std::io::Error::last_os_error();
        if first_call_succeeded
            || required_size == 0
            || sizing_error.raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER)
        {
            return Err(if first_call_succeeded {
                std::io::Error::other(
                    "attribute-list sizing unexpectedly succeeded without a buffer",
                )
            } else {
                sizing_error
            });
        }

        let storage_words = required_size.div_ceil(std::mem::size_of::<usize>());
        let handles = handles.into_iter().collect::<Vec<_>>().into_boxed_slice();
        let mut attribute_list = Self {
            storage: vec![0usize; storage_words],
            _handles: handles,
            _jobs: jobs,
            initialized: false,
        };

        if unsafe {
            InitializeProcThreadAttributeList(attribute_list.as_mut_raw(), 2, 0, &mut required_size)
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        attribute_list.initialized = true;

        let attribute_list_ptr = attribute_list.as_mut_raw();
        if unsafe {
            UpdateProcThreadAttribute(
                attribute_list_ptr,
                0,
                PROC_THREAD_ATTRIBUTE_HANDLE_LIST,
                attribute_list._handles.as_ptr().cast(),
                std::mem::size_of_val(attribute_list._handles.as_ref()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }

        // Assignment is atomic with process creation: no child can escape in
        // the interval between CreateProcess and a later AssignProcessToJob.
        if unsafe {
            UpdateProcThreadAttribute(
                attribute_list_ptr,
                0,
                windows_sys::Win32::System::Threading::PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
                attribute_list._jobs.as_ptr().cast(),
                std::mem::size_of_val(attribute_list._jobs.as_ref()),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(attribute_list)
    }

    fn as_mut_raw(&mut self) -> *mut std::ffi::c_void {
        self.storage.as_mut_ptr().cast()
    }
}

impl Drop for ProcThreadAttributeList {
    fn drop(&mut self) {
        if self.initialized {
            let attribute_list = self.as_mut_raw();
            unsafe { DeleteProcThreadAttributeList(attribute_list) };
        }
    }
}
