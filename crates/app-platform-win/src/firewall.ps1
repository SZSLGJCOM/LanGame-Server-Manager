[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$ErrorActionPreference = 'Stop'
$rules = $rulesJson | ConvertFrom-Json
$displayNames = [Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($spec in $rules) { [void]$displayNames.Add([string]$spec.rule_name) }

function Find-InstanceRule($all, $spec) {
  $found = @($all | Where-Object { $_.DisplayName -ieq $spec.rule_name })
  if ($found.Count -gt 1 -or ($found.Count -eq 1 -and
      ($found[0].DisplayName -cne $spec.rule_name -or $found[0].Group -cne 'LanGame Server Manager'))) {
    throw 'rule_conflict'
  }
  if ($found.Count -eq 1) { return $found[0] }
  return $null
}

function Find-RuleIdentity($all, [string]$name) {
  $found = @($all | Where-Object { $_.Name -ceq $name })
  if ($found.Count -gt 1) { throw 'verification_failed' }
  if ($found.Count -eq 1) { return $found[0] }
  return $null
}

function Index-RuleFilters($filters, $wanted) {
  $index = [Collections.Generic.Dictionary[string,object]]::new([StringComparer]::Ordinal)
  foreach ($filter in $filters) {
    $id = [string]$filter.InstanceID
    if ($wanted.Contains($id)) {
      if ($index.ContainsKey($id)) { throw 'verification_failed' }
      $index.Add($id, $filter)
    }
  }
  return $index
}

function Test-PermissionDenied($errorRecord) {
  return ($errorRecord.CategoryInfo.Category -eq 'PermissionDenied' -or
    $errorRecord.Exception -is [UnauthorizedAccessException])
}

function Read-RuleFilters([string]$command, $targets, $wanted) {
  try {
    $filters = @(& $command -PolicyStore PersistentStore -ErrorAction Stop)
  } catch {
    if (!(Test-PermissionDenied $_)) { throw }
    # Some native GetAll queries require elevation even though the same
    # ordinary token can read each associated rule's complete filter. Use the
    # documented single-CimInstance API only for a proven permission refusal.
    # Each target is from this snapshot, bounded by the validated request and
    # its pending replacements. Errors from association still fail the read.
    $filters = @(foreach ($rule in $targets) {
      & $command -AssociatedNetFirewallRule $rule -PolicyStore PersistentStore -ErrorAction Stop
    })
  }
  return Index-RuleFilters $filters $wanted
}

function Read-InstanceSnapshot($all) {
  $wanted = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
  $targets = @()
  foreach ($rule in $all) {
    if ($displayNames.Contains([string]$rule.DisplayName)) {
      # NetSecurity's filters are one-to-one with a rule. Native provider checks
      # confirm each filter's exact InstanceID equals the rule's Name/InstanceID.
      # Do not associate by result order, friendly names or guessed prefixes.
      $id = [string]$rule.Name
      if ([string]::IsNullOrEmpty($id) -or [string]$rule.InstanceID -cne $id -or !$wanted.Add($id)) {
        throw 'verification_failed'
      }
      $targets += $rule
    }
  }
  $snapshot = @{ Rules=$all }
  if ($wanted.Count -eq 0) { return $snapshot }
  # Prefer one GetAll query per type; on permission refusal read only the
  # targeted rules through its equivalent association API, without elevation.
  # Both paths retain every field and associate only by exact CIM identity.
  # https://learn.microsoft.com/en-us/powershell/module/netsecurity/get-netfirewallrule
  # https://learn.microsoft.com/en-us/powershell/module/netsecurity/get-netfirewallportfilter
  # The command names are compiled constants, never request data.
  $snapshot.Port = Read-RuleFilters 'Get-NetFirewallPortFilter' $targets $wanted
  $snapshot.Address = Read-RuleFilters 'Get-NetFirewallAddressFilter' $targets $wanted
  $snapshot.Application = Read-RuleFilters 'Get-NetFirewallApplicationFilter' $targets $wanted
  $snapshot.Service = Read-RuleFilters 'Get-NetFirewallServiceFilter' $targets $wanted
  $snapshot.Interface = Read-RuleFilters 'Get-NetFirewallInterfaceFilter' $targets $wanted
  $snapshot.InterfaceType = Read-RuleFilters 'Get-NetFirewallInterfaceTypeFilter' $targets $wanted
  $snapshot.Security = Read-RuleFilters 'Get-NetFirewallSecurityFilter' $targets $wanted
  return $snapshot
}

function Test-InstanceRule($rule, $spec, $snapshot) {
  if ($null -eq $rule -or [string]$rule.Enabled -ne 'True' -or
      [string]$rule.Direction -ne 'Inbound' -or [string]$rule.Action -ne 'Allow' -or
      [string]$rule.Profile -ne 'Any' -or [string]$rule.EdgeTraversalPolicy -ne 'Block') { return $false }
  $id = [string]$rule.Name
  foreach ($type in @('Port','Address','Application','Service','Interface','InterfaceType','Security')) {
    if ($null -eq $snapshot[$type] -or !$snapshot[$type].ContainsKey($id)) { return $false }
  }
  $ports=$snapshot.Port[$id]; $addresses=$snapshot.Address[$id]
  $applications=$snapshot.Application[$id]; $services=$snapshot.Service[$id]
  $interfaces=$snapshot.Interface[$id]; $interfaceTypes=$snapshot.InterfaceType[$id]
  $security=$snapshot.Security[$id]
  $protocol = if ($spec.protocol -eq 'UDP') { '17' } else { '6' }
  return ([string]$ports.LocalPort -eq [string]$spec.local_port -and
    ([string]$ports.Protocol -eq $spec.protocol -or [string]$ports.Protocol -eq $protocol) -and
    [string]$ports.RemotePort -eq 'Any' -and
    [string]$addresses.LocalAddress -eq $spec.local_address -and [string]$addresses.RemoteAddress -eq 'Any' -and
    [string]$applications.Program -eq 'Any' -and
    ([string]::IsNullOrEmpty([string]$applications.Package) -or [string]$applications.Package -eq 'Any') -and
    [string]$services.Service -eq 'Any' -and [string]$interfaces.InterfaceAlias -eq 'Any' -and
    [string]$interfaceTypes.InterfaceType -eq 'Any' -and
    [string]$security.Authentication -eq 'NotRequired' -and [string]$security.Encryption -eq 'NotRequired' -and
    [string]$security.OverrideBlockRules -eq 'False' -and
    ([string]::IsNullOrEmpty([string]$security.LocalUser) -or [string]$security.LocalUser -eq 'Any') -and
    ([string]::IsNullOrEmpty([string]$security.RemoteUser) -or [string]$security.RemoteUser -eq 'Any') -and
    ([string]::IsNullOrEmpty([string]$security.RemoteMachine) -or [string]$security.RemoteMachine -eq 'Any'))
}

function Test-CreatedIdentity($rule, $entry) {
  return ($null -ne $rule -and $rule.Name -ceq $entry.Name -and
    $rule.DisplayName -ceq $entry.Spec.rule_name -and $rule.Group -ceq 'LanGame Server Manager' -and
    $rule.Description -ceq $entry.Description)
}

function Rule-Result($spec, [string]$status) {
  [pscustomobject]@{rule_name=$spec.rule_name;protocol=$spec.protocol;local_port=$spec.local_port;
    local_address=$spec.local_address;status=$status;message=$null}
}

$pending = @()
try {
  # Check ownership for the entire batch before loading filters or mutating.
  $all = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
  $existing = @{}
  foreach ($spec in $rules) { $existing[$spec.rule_name] = Find-InstanceRule $all $spec }
  $snapshot = Read-InstanceSnapshot $all
  $repair = @()
  $statuses = @{}
  foreach ($spec in $rules) {
    $statuses[$spec.rule_name] = 'existing'
    if (!(Test-InstanceRule $existing[$spec.rule_name] $spec $snapshot)) { $repair += $spec }
  }
  if ($repair.Count -eq 0) {
    $results = @($rules | ForEach-Object { Rule-Result $_ 'existing' })
    ConvertTo-Json -InputObject @{state='ready';rules=$results} -Compress -Depth 4
    return
  }
  if (!$apply) {
    ConvertTo-Json -InputObject @{state='repair';rules=@()} -Compress
    return
  }

  # Refresh the whole ownership boundary immediately before creation. Reject a
  # replaced identity rather than silently adopting a change made by another owner.
  $all = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
  foreach ($spec in $rules) {
    $current = Find-InstanceRule $all $spec
    if ([string]$current.Name -cne [string]$existing[$spec.rule_name].Name) { throw 'rule_conflict' }
  }
  foreach ($spec in $repair) {
    $old = $existing[$spec.rule_name]
    $name = [string]$spec.rule_name
    $protocol = [string]$spec.protocol
    $localPort = [int]$spec.local_port
    $localAddress = [string]$spec.local_address
    $createdName = 'LanGame-' + [guid]::NewGuid().ToString('N')
    $description = 'Created by LanGame Server Manager; operation ' + $createdName
    $pending += @{Name=$createdName;Description=$description;Spec=$spec;Validated=$false;Committed=$false;
      OldName=$(if ($null -ne $old) { [string]$old.Name } else { $null })}
    New-NetFirewallRule -Name $createdName -DisplayName $name -Direction Inbound -Action Allow -Protocol $protocol -LocalPort $localPort -LocalAddress $localAddress -Profile Any -Enabled True -Group 'LanGame Server Manager' -Description $description -ErrorAction Stop | Out-Null
  }

  # Old rules remain until every created identity and its complete scope have
  # passed one new snapshot. A middle creation/validation failure can roll back
  # precisely this operation's additions while retaining all old rules.
  $snapshot = Read-InstanceSnapshot @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
  foreach ($entry in $pending) {
    $created = Find-RuleIdentity $snapshot.Rules $entry.Name
    if (!(Test-CreatedIdentity $created $entry) -or !(Test-InstanceRule $created $entry.Spec $snapshot)) {
      throw 'verification_failed'
    }
    $entry.Validated = $true
  }
  # Refresh identity/ownership again before deleting any old rule.
  $all = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
  foreach ($entry in $pending) {
    if (!(Test-CreatedIdentity (Find-RuleIdentity $all $entry.Name) $entry)) { throw 'rule_conflict' }
    if ($null -ne $entry.OldName) {
      $old = Find-RuleIdentity $all $entry.OldName
      if ($null -eq $old -or $old.DisplayName -cne $entry.Spec.rule_name -or $old.Group -cne 'LanGame Server Manager') {
        throw 'rule_conflict'
      }
    }
  }
  foreach ($entry in $pending) {
    if ($null -ne $entry.OldName) {
      (Find-RuleIdentity $all $entry.OldName) | Remove-NetFirewallRule -ErrorAction Stop
      $statuses[$entry.Spec.rule_name] = 'replaced'
    } else { $statuses[$entry.Spec.rule_name] = 'created'; $entry.Committed=$true }
  }
  $all = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
  foreach ($entry in $pending) {
    if ($null -ne $entry.OldName -and $null -ne (Find-RuleIdentity $all $entry.OldName)) { throw 'verification_failed' }
  }
  $pending = @()
  $snapshot = Read-InstanceSnapshot $all
  foreach ($spec in $rules) {
    if (!(Test-InstanceRule (Find-InstanceRule $all $spec) $spec $snapshot)) { throw 'verification_failed' }
  }
  $results = @($rules | ForEach-Object { Rule-Result $_ $statuses[$_.rule_name] })
  ConvertTo-Json -InputObject @{state='ready';rules=$results} -Compress -Depth 4
} catch {
  # Only stable codes cross IPC. Never echo CIM details, commands or paths.
  $code = 'apply_failed'
  if ($_.Exception.Message -ceq 'rule_conflict') { $code='rule_conflict' }
  elseif ($_.Exception.Message -ceq 'verification_failed') { $code='verification_failed' }
  elseif (Test-PermissionDenied $_) { $code='permission_denied' }
  if ($pending.Count -gt 0) {
    try {
      $snapshot = Read-InstanceSnapshot @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
      $rollback = @()
      foreach ($entry in $pending) {
        $current = Find-RuleIdentity $snapshot.Rules $entry.Name
        if ($null -eq $current -or $entry.Committed) { continue }
        if (!(Test-CreatedIdentity $current $entry) -or
            ($entry.Validated -and !(Test-InstanceRule $current $entry.Spec $snapshot))) { throw 'cleanup_failed' }
        $keep = $false
        if ($null -ne $entry.OldName) {
          $old = Find-RuleIdentity $snapshot.Rules $entry.OldName
          if ($null -ne $old -and ($old.DisplayName -cne $entry.Spec.rule_name -or $old.Group -cne 'LanGame Server Manager')) {
            throw 'cleanup_failed'
          }
          # Removal can succeed before returning an error. Keep a validated
          # replacement if its old identity has actually disappeared.
          $keep = $null -eq $old -and $entry.Validated
        }
        if (!$keep) { $rollback += $current }
      }
      # Validate the rollback set before its first mutation. Unvalidated additions
      # still require their generated Name, group and exact operation marker.
      foreach ($rule in $rollback) { $rule | Remove-NetFirewallRule -ErrorAction Stop }
      $all = @(Get-NetFirewallRule -PolicyStore PersistentStore -ErrorAction Stop)
      foreach ($rule in $rollback) {
        if ($null -ne (Find-RuleIdentity $all $rule.Name)) { throw 'cleanup_failed' }
      }
    } catch { $code='cleanup_failed' }
  }
  ConvertTo-Json -InputObject @{state=$code;rules=@()} -Compress
}
