#include "../save_completion.hpp"
#include <cstdio>
#include <cstdlib>
using namespace dragon_save;
namespace {
unsigned checks{};
void check(bool value, const char* name) {
    ++checks;
    if (!value) { std::fprintf(stderr,"FAIL %s\n",name); std::exit(1); }
}
Completion root(unsigned players = 0) {
    Completion value(100);
    value.enter_root();
    for (unsigned index = 0; index < players; ++index) value.player(static_cast<std::int32_t>(10 + index));
    value.begin_world_stage();
    return value;
}
void write(Completion& value) {
    value.enter_slot(); value.writer(); value.close_writer(true);
    value.publish(true,true); value.leave_slot(true);
}
void complete(Completion& value) {
    value.world_handle(1000); value.result(Kind::world,1000,true);
    for (std::size_t index=0;index<value.player_count;++index)
        value.result(Kind::player,static_cast<std::int32_t>(10+index),true);
    value.enter_aggregate(value.player_count+1,true); value.leave_aggregate();
    value.leave_root(value.player_count+1);
}
}
int main() {
    { auto value=root(); write(value); complete(value); check(value.status==Status::completed,"world writer, result, final aggregate, root all returned"); }
    { auto value=root(256); write(value); complete(value); check(value.status==Status::completed,"all 256 native player handles required"); }
    { auto value=root(); complete(value); check(value.status!=Status::completed,"aggregate true cannot replace disk writer"); }
    { auto value=root(); value.enter_slot(); value.writer(); value.publish(true,true); value.leave_slot(true); complete(value); check(value.status!=Status::completed,"publication cannot replace close"); }
    { auto value=root(); value.enter_slot(); value.writer(); value.close_writer(false); value.publish(true,true); value.leave_slot(true); complete(value); check(value.status==Status::failed,"native close failure retained"); }
    { auto value=root(); value.enter_slot(); value.writer(); value.close_writer(true); value.publish(true,false); value.leave_slot(true); complete(value); check(value.status==Status::unknown,"publication readback mismatch retained"); }
    { auto value=root(); value.enter_slot(); value.writer(); value.close_writer(true); value.publish(false,true); value.leave_slot(true); complete(value); check(value.status==Status::failed,"native publication failure retained"); }
    { auto value=root(); write(value); value.fail(); complete(value); check(value.status==Status::failed,"version-backup copy failure cannot be hidden by game bool"); }
    { auto value=root(1); write(value); value.world_handle(1000); value.result(Kind::world,1000,true); value.enter_aggregate(2,true); value.leave_aggregate(); value.leave_root(2); check(value.status==Status::unknown,"missing player result cannot finish"); }
    { auto value=root(1); write(value); value.world_handle(1000); value.result(Kind::world,1000,true); value.result(Kind::player,10,false); value.enter_aggregate(2,true); value.leave_aggregate(); value.leave_root(2); check(value.status==Status::failed,"player failure cannot be overwritten by aggregate"); }
    { auto value=root(); write(value); value.world_handle(1000); value.result(Kind::world,1000,true); value.enter_aggregate(1,true); value.leave_root(1); check(value.status==Status::dispatching,"aggregate callback stack retains debt"); value.leave_aggregate(); check(value.status==Status::completed,"aggregate actual return drains last debt"); }
    { auto value=root(); write(value); value.enter_native(); complete(value); check(value.status==Status::dispatching,"nested callback retains debt"); value.leave_native(); check(value.status==Status::completed,"nested actual return closes operation"); }
    { auto value=root(); write(value); value.tick(120100); complete(value); check(value.status==Status::unknown && value.issue==Issue::timed_out,"late success never revives timed out request"); }
    { auto value=root(); write(value); value.unknown(Issue::identity_changed); complete(value); check(value.status==Status::unknown,"world identity loss sticky"); }
    { auto value=root(); write(value); value.enter_root(); complete(value); check(value.status==Status::unknown,"foreign overlapping root never adopted"); }
    { auto value=root(); write(value); value.world_handle(1000); value.result(Kind::world,1001,true); check(value.status==Status::unknown,"foreign world handle rejected"); }
    { auto value=root(1); value.player(10); check(value.status==Status::unknown,"duplicate player handle rejected"); }
    { auto value=root(1); value.world_handle(10); check(value.status==Status::unknown,"cross kind handle collision rejected"); }
    { auto value=root(); value.world_handle(1000); value.result(Kind::world,1000,true); value.result(Kind::world,1000,true); check(value.status==Status::unknown,"duplicate terminal callback rejected"); }
    { auto value=root(); write(value); complete(value); value.tick(999999); check(value.status==Status::completed,"completed receipt not changed by later clock"); }
    { auto value=root(257); check(value.status==Status::unknown,"fanout bound rejects overflow"); }
    { auto value=root(); write(value); value.world_handle(1000); value.result(Kind::world,1000,true); value.enter_aggregate(0,true); check(value.status==Status::unknown,"empty aggregate is not success"); }
    { auto value=root(); write(value); value.world_handle(1000); value.result(Kind::world,1000,true); value.enter_aggregate(1,false); check(value.status==Status::failed,"final aggregate native failure"); }
    { auto value=root(); write(value); value.leave_root(2); check(value.status==Status::unknown,"root expected count must match owned fanout"); }
    std::printf("PASS %u Dragonwilds completion state checks (constructed events, not native integration)\n",checks);
}
