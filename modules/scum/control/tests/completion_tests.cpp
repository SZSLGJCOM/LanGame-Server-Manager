#include "../completion.hpp"
#include "../sql_completion.hpp"
#include <iostream>
#include <string_view>
namespace {
unsigned checks{};
void check(bool condition){if(!condition)throw std::runtime_error("completion_check");++checks;}
void check_issue(const scum_save::Completion& value,std::string_view expected){if(value.issue()!=expected)throw std::runtime_error("first_rejection_issue_overwritten");++checks;}
scum_save::Completion published(){scum_save::Completion value;value.stage(1);value.stage(2);value.stage(4);value.published(7);value.root_returned();return value;}
template<class F>void rejects(F action){bool rejected{};try{action();}catch(const std::runtime_error&){rejected=true;}check(rejected);}
}
int main(){try {
    using namespace scum_save;
    auto normal=published();check(!normal.confirmed(true,true));normal.batch_entered({7});normal.batch_returned({7});
    check(!normal.confirmed(true,true));normal.pump_entered({7});normal.pump_returned({7});check(normal.confirmed(true,true));
    check(!normal.confirmed(false,true));check(!normal.confirmed(true,false));
    auto race=published();race.batch_entered({7});race.pump_entered({7});race.pump_returned({7});check(!race.confirmed(true,true));race.batch_returned({7});check(race.confirmed(true,true));
    auto cancelled=normal;cancelled.cancelled(7);check(!cancelled.confirmed(true,true));
    auto foreign=published();rejects([&]{foreign.batch_entered({8});});check(!foreign.confirmed(true,true));
    auto duplicate=published();rejects([&]{duplicate.published(7);});
    auto child=published();child.batch_entered({7});child.batch_returned({7});child.pump_entered({7});child.published(8);child.pump_returned({7});
    check(!child.confirmed(true,true));child.batch_entered({8});child.batch_returned({8});child.pump_entered({8});child.pump_returned({8});check(child.confirmed(true,true));
    SqlCompletion sql;sql.step(SqlKind::begin,101,true,false);sql.step(SqlKind::write,101,false,false);check(!sql.closed());sql.step(SqlKind::commit,101,false,true);check(!sql.closed());sql.readback_verified();check(sql.closed());
    sql.step(SqlKind::write,101,true,true);check(!sql.closed());sql.readback_verified();check(sql.closed());
    auto bind=sql;bind.api_result(25);bind.step(SqlKind::write,101,true,true);bind.readback_verified();check(!bind.closed());
    auto rollback=sql;rollback.step(SqlKind::rollback,101,false,true);rollback.readback_verified();check(!rollback.closed());
    SqlCompletion borrowed;borrowed.step(SqlKind::commit,101,false,true);borrowed.readback_verified();check(!borrowed.closed());
    Completion fresh;check_issue(fresh,"native_save_pending");check(!fresh.rejected()&&!fresh.failed());
    auto first_unknown=normal;first_unknown.unknown("native_batch_foreign_sequence");
    check_issue(first_unknown,"native_batch_foreign_sequence");check(first_unknown.rejected()&&!first_unknown.failed()&&!first_unknown.confirmed(true,true));
    first_unknown.unknown("foreign_native_sql");check_issue(first_unknown,"native_batch_foreign_sequence");
    first_unknown.unknown("native_save_timeout");check_issue(first_unknown,"native_batch_foreign_sequence");
    first_unknown.failed("native_sql_write_failed");check_issue(first_unknown,"native_batch_foreign_sequence");
    check(first_unknown.failed()&&first_unknown.native_settled()&&!first_unknown.confirmed(true,true));
    auto first_failure=normal;first_failure.failed("native_sql_write_failed");
    first_failure.failed("native_sql_close_failed");first_failure.unknown("foreign_native_sql");
    check_issue(first_failure,"native_sql_write_failed");check(first_failure.failed()&&first_failure.rejected()&&!first_failure.confirmed(true,true));
    auto first_contract=published();rejects([&]{first_contract.batch_entered({8});});
    first_contract.unknown("native_batch_ownership");first_contract.unknown("foreign_native_sql");
    check_issue(first_contract,"native_batch_foreign_sequence");check(first_contract.rejected()&&!first_contract.failed()&&!first_contract.native_settled()&&!first_contract.confirmed(true,true));
    auto first_cancel=normal;first_cancel.cancelled(7);first_cancel.unknown("native_profile_changed");
    check_issue(first_cancel,"native_task_cancelled");check(first_cancel.rejected()&&!first_cancel.failed()&&!first_cancel.confirmed(true,true));
    std::cout<<"SCUM causal completion policy checks="<<checks<<"\n";return 0;
}catch(const std::exception& error){std::cerr<<error.what()<<"\n";return 1;}}
