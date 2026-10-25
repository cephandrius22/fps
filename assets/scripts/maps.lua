-- Entity classes used by the maps in assets/maps/. A map entity
-- picks one with a "script" key, e.g. "script" "MapGreeter" on a
-- trigger brush in maps/test.map.

-- A trigger that greets the player the first time they walk in.
MapGreeter = {}

function MapGreeter:on_enter()
    if self.greeted then
        return
    end

    self.greeted = true
    world.log("You found room B. The door behind you reopens as you leave.")
end
